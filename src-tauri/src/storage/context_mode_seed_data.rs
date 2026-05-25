use crate::error::AppResult;
use crate::storage::database::Database;
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

const SELECT_COLS: &str =
    "id, name, description, icon, color, llm_prompt, sort_order, is_builtin, created_at, updated_at, writing_style";

pub fn seed_builtin_context_modes(db: &Database) -> AppResult<()> {
    seed_programming_mode(db)?;
    seed_business_sales_mode(db)?;
    seed_medical_mode(db)?;
    seed_legal_mode(db)?;
    Ok(())
}

/// Seed global dictionary entries for common Whisper misrecognitions.
///
/// These are phonetic/homophone errors that Whisper commonly makes.
/// Entries are inserted idempotently — skipped if the phrase already exists
/// (the user may have customized or deleted them).
pub fn seed_general_dictionary(db: &Database) -> AppResult<()> {
    let corrections: &[(&str, &str)] = &[
        // Common Whisper mishears — technology
        ("AY", "AI"),
        // Contractions / informal speech
        ("gonna", "going to"),
        ("wanna", "want to"),
        ("gotta", "got to"),
        ("kinda", "kind of"),
        ("shoulda", "should have"),
        ("coulda", "could have"),
        ("woulda", "would have"),
        ("dunno", "don't know"),
        ("lemme", "let me"),
        ("gimme", "give me"),
    ];

    let conn = db.conn()?;
    let now = Utc::now().to_rfc3339();

    for (phrase, replacement) in corrections {
        // Skip if this phrase already exists (global = mode_id IS NULL)
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM dictionary_entries \
                 WHERE LOWER(phrase) = LOWER(?1) AND mode_id IS NULL",
                params![phrase],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !exists {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO dictionary_entries (id, phrase, replacement, is_enabled, created_at, mode_id) \
                 VALUES (?1, ?2, ?3, 1, ?4, NULL)",
                params![id, phrase, replacement, now],
            )?;
        }
    }

    Ok(())
}

struct BuiltinModeSeed<'a> {
    name: &'a str,
    description: &'a str,
    icon: &'a str,
    color: &'a str,
    additions: &'a str,
    sort_order: i32,
    writing_style: &'a str,
    dictionary: &'a [(&'a str, &'a str)],
    snippets: &'a [(&'a str, &'a str, &'a str)],
}

fn seed_builtin_mode(db: &Database, seed: BuiltinModeSeed<'_>) -> AppResult<()> {
    let conn = db.conn()?;
    let existing: Option<(String, i64)> = conn
        .query_row(
            "SELECT cm.id, (SELECT COUNT(*) FROM dictionary_entries WHERE mode_id = cm.id)
             FROM context_modes cm WHERE cm.name = ?1",
            params![seed.name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let (id, needs_entries) = match existing {
        Some((id, count)) if count > 0 => {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE context_modes SET llm_prompt = ?1, updated_at = ?2 \
                 WHERE id = ?3 AND is_builtin = 1",
                params![seed.additions, now, id],
            )?;
            return Ok(());
        }
        Some((id, _)) => (id, true),
        None => (Uuid::new_v4().to_string(), false),
    };

    let now = Utc::now().to_rfc3339();
    if !needs_entries {
        conn.execute(
            &format!("INSERT INTO context_modes ({SELECT_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
            params![
                id,
                seed.name,
                seed.description,
                seed.icon,
                seed.color,
                seed.additions,
                seed.sort_order,
                true,
                now,
                now,
                seed.writing_style,
            ],
        )?;
    }

    for (phrase, replacement) in seed.dictionary {
        let entry_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO dictionary_entries (id, phrase, replacement, is_enabled, created_at, mode_id)
             VALUES (?1, ?2, ?3, 1, ?4, ?5)",
            params![entry_id, phrase, replacement, now, id],
        )?;
    }

    for (trigger, content, description) in seed.snippets {
        let snippet_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO snippets (id, trigger_text, content, description, is_enabled, created_at, mode_id)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![snippet_id, trigger, content, description, now, id],
        )?;
    }

    Ok(())
}

/// Mode-specific additions for Programming mode (appended to base rules).
const PROGRAMMING_ADDITIONS: &str = "\
- Recognize programming terms, syntax, and jargon:
  - Language names: JavaScript, TypeScript, Python, Rust, Go, C++, etc.
  - Frameworks/libs: React, Next.js, Tauri, Node, Express, Django, Flask, etc.
  - Concepts: API, REST, GraphQL, SQL, NoSQL, ORM, CLI, GUI, SDK, IDE, CI/CD
  - Operations: git push, git commit, npm install, cargo build, pip install
  - Types/patterns: async/await, callback, promise, mutex, trait, interface, enum
  - Symbols: When the speaker says \"dot\" in a code context, use \".\" — e.g. \"console dot log\" → \"console.log\"
  - Casing: Preserve camelCase, PascalCase, snake_case, and SCREAMING_SNAKE when the speaker clearly intends them
- Do NOT wrap output in code blocks or backticks — output plain text only";

/// Seed the Programming/Coding builtin mode if it doesn't exist,
/// or backfill its dictionary/snippets if they're missing (e.g. from
/// a previous launch that created the mode but not its entries).
fn seed_programming_mode(db: &Database) -> AppResult<()> {
    let dict_entries: &[(&str, &str)] = &[
        // Language names
        ("java script", "JavaScript"),
        ("type script", "TypeScript"),
        ("pie thon", "Python"),
        ("c sharp", "C#"),
        ("f sharp", "F#"),
        ("go lang", "Golang"),
        ("rust lang", "Rust"),
        ("c plus plus", "C++"),
        ("objective c", "Objective-C"),
        ("kotlin", "Kotlin"),
        // Frameworks & runtimes
        ("node js", "Node.js"),
        ("next js", "Next.js"),
        ("vue js", "Vue.js"),
        ("nuxt js", "Nuxt.js"),
        ("express js", "Express.js"),
        ("deno", "Deno"),
        ("django", "Django"),
        ("flask", "Flask"),
        ("fast api", "FastAPI"),
        ("spring boot", "Spring Boot"),
        ("dot net", ".NET"),
        // Tools & platforms
        ("get hub", "GitHub"),
        ("get lab", "GitLab"),
        ("bit bucket", "Bitbucket"),
        ("v s code", "VS Code"),
        ("vis code", "VS Code"),
        ("pie charm", "PyCharm"),
        ("intellij", "IntelliJ"),
        ("web pack", "Webpack"),
        ("docker", "Docker"),
        ("kubernetes", "Kubernetes"),
        ("terraform", "Terraform"),
        ("jenkins", "Jenkins"),
        // Data formats & databases
        ("jason", "JSON"),
        ("yaml", "YAML"),
        ("to ml", "TOML"),
        ("my sequel", "MySQL"),
        ("post gres", "Postgres"),
        ("postgres q l", "PostgreSQL"),
        ("mongo db", "MongoDB"),
        ("dynamo db", "DynamoDB"),
        ("fire base", "Firebase"),
        ("redis", "Redis"),
        ("sequel lite", "SQLite"),
        ("elastic search", "Elasticsearch"),
        // Libraries
        ("num pie", "NumPy"),
        ("sci pie", "SciPy"),
        ("pandas", "pandas"),
        ("tensor flow", "TensorFlow"),
        ("pie torch", "PyTorch"),
        // Concepts (common compound-word corrections)
        ("end point", "endpoint"),
        ("back end", "backend"),
        ("front end", "frontend"),
        ("dev ops", "DevOps"),
        ("web hook", "webhook"),
        ("web socket", "WebSocket"),
        ("cron job", "cron job"),
        ("name space", "namespace"),
        ("type def", "typedef"),
        // AI / Agentic
        ("open ai", "OpenAI"),
        ("chat gpt", "ChatGPT"),
        ("g p t", "GPT"),
        ("lang chain", "LangChain"),
        ("llama index", "LlamaIndex"),
        ("anthropic", "Anthropic"),
        ("hugging face", "Hugging Face"),
        ("mid journey", "Midjourney"),
        ("stable diffusion", "Stable Diffusion"),
    ];

    let snippet_entries: &[(&str, &str, &str)] = &[
        // trigger, content, description
        ("shebang bash", "#!/usr/bin/env bash", "Bash shebang line"),
        (
            "shebang python",
            "#!/usr/bin/env python3",
            "Python shebang line",
        ),
        (
            "shebang node",
            "#!/usr/bin/env node",
            "Node.js shebang line",
        ),
        ("todo comment", "// TODO: ", "TODO comment marker"),
        ("fixme comment", "// FIXME: ", "FIXME comment marker"),
        ("hack comment", "// HACK: ", "HACK comment marker"),
        ("note comment", "// NOTE: ", "NOTE comment marker"),
    ];

    seed_builtin_mode(
        db,
        BuiltinModeSeed {
            name: "Programming",
            description: "Optimized for coding and software development",
            icon: "code",
            color: "blue",
            additions: PROGRAMMING_ADDITIONS,
            sort_order: 1,
            writing_style: "formal",
            dictionary: dict_entries,
            snippets: snippet_entries,
        },
    )
}

/// Mode-specific additions for Business & Sales mode.
const BUSINESS_SALES_ADDITIONS: &str = "\
- Recognize business, sales, and CRM terminology:
  - Metrics: ARR, MRR, ACV, TCV, CAC, LTV, CLV, NRR, GRR, NPS, CSAT, churn rate, burn rate, runway
  - Sales stages: MQL, SQL, SAL, SAO, discovery, demo, negotiation, closed-won, closed-lost, pipeline
  - Roles: SDR, BDR, AE, AM, CSM, VP of Sales, CRO, CMO, CFO, CEO, CTO, COO
  - CRM & tools: Salesforce, HubSpot, Outreach, Gong, ZoomInfo, LinkedIn, Slack, Zoom, Teams
  - SaaS concepts: ARR, MRR, churn, upsell, cross-sell, expansion revenue, logo retention, net retention
  - Deal terms: SOW, MSA, NDA, SLA, RFP, RFQ, RFI, PO, invoice, renewal, multi-year
  - Business acronyms: ROI, KPI, OKR, P&L, EBITDA, QBR, QoQ, YoY, MoM, WoW, EOD, EOM, EOQ, EOY
  - Frameworks: BANT, MEDDIC, MEDDPICC, SPIN, Challenger, SPICED, value selling
- Format currency amounts with $ symbol when clearly dictated (e.g. \"ten thousand dollars\" → \"$10,000\")
- Preserve professional email tone — do not make language overly casual or overly formal";

/// Seed the Business & Sales builtin mode.
fn seed_business_sales_mode(db: &Database) -> AppResult<()> {
    let dict_entries: &[(&str, &str)] = &[
        // CRM platforms
        ("sales force", "Salesforce"),
        ("hub spot", "HubSpot"),
        ("pipe drive", "Pipedrive"),
        ("zoom info", "ZoomInfo"),
        ("out reach", "Outreach"),
        ("sales loft", "SalesLoft"),
        ("go high level", "GoHighLevel"),
        ("mail chimp", "Mailchimp"),
        ("constant contact", "Constant Contact"),
        ("send grid", "SendGrid"),
        // Communication tools
        ("microsoft teams", "Microsoft Teams"),
        ("google meet", "Google Meet"),
        ("calendly", "Calendly"),
        ("loom", "Loom"),
        ("doc u sign", "DocuSign"),
        ("docu sign", "DocuSign"),
        // Business metrics (spoken as letters)
        ("a r r", "ARR"),
        ("m r r", "MRR"),
        ("a c v", "ACV"),
        ("t c v", "TCV"),
        ("c a c", "CAC"),
        ("l t v", "LTV"),
        ("c l v", "CLV"),
        ("n r r", "NRR"),
        ("g r r", "GRR"),
        ("n p s", "NPS"),
        ("c sat", "CSAT"),
        ("r o i", "ROI"),
        ("k p i", "KPI"),
        ("o k r", "OKR"),
        ("e b i t d a", "EBITDA"),
        ("p and l", "P&L"),
        ("q b r", "QBR"),
        // Time references
        ("e o d", "EOD"),
        ("e o w", "EOW"),
        ("e o m", "EOM"),
        ("e o q", "EOQ"),
        ("e o y", "EOY"),
        ("y o y", "YoY"),
        ("q o q", "QoQ"),
        ("m o m", "MoM"),
        ("w o w", "WoW"),
        // Sales roles
        ("s d r", "SDR"),
        ("b d r", "BDR"),
        ("a e", "AE"),
        ("a m", "AM"),
        ("c s m", "CSM"),
        ("c r o", "CRO"),
        ("c m o", "CMO"),
        ("c f o", "CFO"),
        ("c e o", "CEO"),
        ("c t o", "CTO"),
        ("c o o", "COO"),
        // Sales stages & lead types
        ("m q l", "MQL"),
        ("s q l", "SQL"),
        ("s a l", "SAL"),
        // Document types
        ("s o w", "SOW"),
        ("m s a", "MSA"),
        ("n d a", "NDA"),
        ("s l a", "SLA"),
        ("r f p", "RFP"),
        ("r f q", "RFQ"),
        ("r f i", "RFI"),
        ("p o", "PO"),
        // Methodologies
        ("med dick", "MEDDIC"),
        ("med pic", "MEDDPICC"),
        ("bant", "BANT"),
        ("spin selling", "SPIN Selling"),
        // Common compound corrections
        ("up sell", "upsell"),
        ("cross sell", "cross-sell"),
        ("on boarding", "onboarding"),
        ("off boarding", "offboarding"),
        ("stake holder", "stakeholder"),
        ("touch point", "touchpoint"),
        ("go to market", "go-to-market"),
        ("year over year", "year-over-year"),
        ("quarter over quarter", "quarter-over-quarter"),
    ];

    let snippet_entries: &[(&str, &str, &str)] = &[
        (
            "best regards",
            "Best regards,",
            "Professional email closing",
        ),
        (
            "kind regards",
            "Kind regards,",
            "Professional email closing",
        ),
        (
            "thanks and regards",
            "Thanks and regards,",
            "Professional email closing",
        ),
        (
            "looking forward",
            "Looking forward to hearing from you.",
            "Professional email sign-off",
        ),
        (
            "per our conversation",
            "Per our conversation,",
            "Email reference opener",
        ),
        (
            "please find attached",
            "Please find attached",
            "Attachment reference",
        ),
        (
            "action items",
            "Action Items:\n- ",
            "Meeting action items header",
        ),
        (
            "next steps",
            "Next Steps:\n- ",
            "Follow-up next steps header",
        ),
        (
            "meeting recap",
            "Meeting Recap\nDate: \nAttendees: \nKey Discussion Points:\n- \n\nAction Items:\n- ",
            "Meeting recap template",
        ),
    ];

    seed_builtin_mode(
        db,
        BuiltinModeSeed {
            name: "Business & Sales",
            description: "Optimized for sales, CRM, and business communication",
            icon: "briefcase",
            color: "green",
            additions: BUSINESS_SALES_ADDITIONS,
            sort_order: 2,
            writing_style: "formal",
            dictionary: dict_entries,
            snippets: snippet_entries,
        },
    )
}

/// Mode-specific additions for Medical mode.
const MEDICAL_ADDITIONS: &str = "\
- Recognize medical terminology, drug names, and clinical jargon:
  - Vitals: BP, HR, RR, SpO2, BMI, temp, pulse ox, systolic, diastolic
  - Labs: CBC, BMP, CMP, LFT, TSH, A1C, HbA1c, BUN, creatinine, WBC, RBC, hemoglobin, hematocrit
  - Imaging: MRI, CT, X-ray, ultrasound, PET scan, DEXA, echocardiogram, EKG, ECG, EEG
  - Prescriptions: Preserve dosage formatting (e.g. \"500 mg\", \"10 mL\", \"0.5 mcg\")
  - Latin abbreviations: q.d., b.i.d., t.i.d., q.i.d., p.r.n., q.h.s., a.c., p.c., p.o., IV, IM, SubQ
  - Documentation: SOAP, HPI, ROS, PMH, PSH, assessment, plan, chief complaint, differential diagnosis
  - Conditions: hypertension, diabetes mellitus, COPD, CHF, CAD, DVT, PE, UTI, GERD, MI, CVA, TIA
  - Procedures: intubation, catheterization, biopsy, excision, debridement, lavage, suture, I&D
- Preserve exact medical terminology — do not simplify or substitute clinical terms with lay terms
- Format drug names with proper capitalization (brand names capitalized, generics lowercase)";

/// Seed the Medical builtin mode.
fn seed_medical_mode(db: &Database) -> AppResult<()> {
    let dict_entries: &[(&str, &str)] = &[
        // Vitals & measurements
        ("blood pressure", "blood pressure"),
        ("bee pee", "BP"),
        ("heart rate", "heart rate"),
        ("pulse ox", "pulse ox"),
        ("oh two sat", "O2 sat"),
        ("s p o 2", "SpO2"),
        ("b m i", "BMI"),
        // Lab tests
        ("c b c", "CBC"),
        ("b m p", "BMP"),
        ("c m p", "CMP"),
        ("l f t", "LFT"),
        ("t s h", "TSH"),
        ("a one c", "A1C"),
        ("h b a one c", "HbA1c"),
        ("b u n", "BUN"),
        ("w b c", "WBC"),
        ("r b c", "RBC"),
        ("p s a", "PSA"),
        ("i n r", "INR"),
        ("p t", "PT"),
        ("p t t", "PTT"),
        // Imaging
        ("m r i", "MRI"),
        ("c t scan", "CT scan"),
        ("c t", "CT"),
        ("pet scan", "PET scan"),
        ("e k g", "EKG"),
        ("e c g", "ECG"),
        ("e e g", "EEG"),
        ("e m g", "EMG"),
        ("decks a scan", "DEXA scan"),
        ("echo cardiogram", "echocardiogram"),
        // Documentation
        ("soap note", "SOAP note"),
        ("h p i", "HPI"),
        ("r o s", "ROS"),
        ("p m h", "PMH"),
        ("p s h", "PSH"),
        // Conditions
        ("c o p d", "COPD"),
        ("c h f", "CHF"),
        ("c a d", "CAD"),
        ("d v t", "DVT"),
        ("p e", "PE"),
        ("u t i", "UTI"),
        ("g e r d", "GERD"),
        ("m i", "MI"),
        ("c v a", "CVA"),
        ("t i a", "TIA"),
        ("a fib", "AFib"),
        ("a flutter", "AFlutter"),
        ("hyper tension", "hypertension"),
        ("hypo tension", "hypotension"),
        ("tachy cardia", "tachycardia"),
        ("brady cardia", "bradycardia"),
        ("diabetes mellitus", "diabetes mellitus"),
        // Prescription abbreviations
        ("b i d", "b.i.d."),
        ("t i d", "t.i.d."),
        ("q i d", "q.i.d."),
        ("q d", "q.d."),
        ("p r n", "p.r.n."),
        ("q h s", "q.h.s."),
        ("p o", "p.o."),
        ("i v", "IV"),
        ("i m", "IM"),
        ("sub q", "SubQ"),
        // Common drug names (frequently misheard)
        ("tylenol", "Tylenol"),
        ("advil", "Advil"),
        ("ibuprofen", "ibuprofen"),
        ("acetaminophen", "acetaminophen"),
        ("amoxicillin", "amoxicillin"),
        ("metformin", "metformin"),
        ("lisinopril", "lisinopril"),
        ("atorvastatin", "atorvastatin"),
        ("omeprazole", "omeprazole"),
        ("metoprolol", "metoprolol"),
        ("amlodipine", "amlodipine"),
        ("losartan", "losartan"),
        ("gabapentin", "gabapentin"),
        ("hydrochlorothiazide", "hydrochlorothiazide"),
        ("levothyroxine", "levothyroxine"),
        // Medical systems
        ("epic", "Epic"),
        ("cerner", "Cerner"),
        ("e h r", "EHR"),
        ("e m r", "EMR"),
        ("h i p a a", "HIPAA"),
        ("hippa", "HIPAA"),
    ];

    let snippet_entries: &[(&str, &str, &str)] = &[
        ("soap note template", "S: \nO: \nA: \nP: ", "SOAP note template"),
        ("vitals template", "Vitals:\n  BP: /  mmHg\n  HR:  bpm\n  RR:  breaths/min\n  Temp:  °F\n  SpO2: %", "Vital signs template"),
        ("review of systems", "ROS:\n  Constitutional: \n  HEENT: \n  Cardiovascular: \n  Respiratory: \n  GI: \n  GU: \n  MSK: \n  Neuro: \n  Psych: ", "Review of systems template"),
        ("prescription template", "Rx:\n  Medication: \n  Dose: \n  Route: \n  Frequency: \n  Duration: \n  Refills: \n  Dispense: ", "Prescription template"),
        ("normal exam", "Physical exam within normal limits.", "Normal exam shorthand"),
        ("no acute distress", "Patient is alert and oriented, in no acute distress.", "NAD assessment opener"),
    ];

    seed_builtin_mode(
        db,
        BuiltinModeSeed {
            name: "Medical",
            description: "Optimized for healthcare and clinical documentation",
            icon: "heart",
            color: "red",
            additions: MEDICAL_ADDITIONS,
            sort_order: 3,
            writing_style: "formal",
            dictionary: dict_entries,
            snippets: snippet_entries,
        },
    )
}

/// Mode-specific additions for Legal mode.
const LEGAL_ADDITIONS: &str = "\
- Recognize legal terminology, Latin phrases, and citation formats:
  - Document types: brief, motion, memorandum, affidavit, deposition, subpoena, complaint, answer, stipulation
  - Court terms: plaintiff, defendant, counsel, jurisdiction, venue, discovery, voir dire, arraignment
  - Latin phrases: habeas corpus, prima facie, pro bono, amicus curiae, certiorari, stare decisis, res judicata, mens rea, actus reus, de facto, de jure, ex parte, in camera, inter alia, per curiam, pro se, sua sponte, sub judice, voir dire
  - Citations: Preserve legal citation formats — e.g. \"Section 230\", \"42 U.S.C.\", \"Fed. R. Civ. P.\", \"Rule 12(b)(6)\"
  - Contract terms: indemnification, force majeure, severability, arbitration, liquidated damages, covenant, warranty, representation
  - Entities: SCOTUS, DOJ, SEC, FTC, IRS, USPTO, FDA, EEOC, OSHA, NLRB
  - Roles: J.D., Esq., partner, associate, paralegal, of counsel, general counsel, in-house counsel
- Preserve formal legal writing style — do not simplify legal terms to plain language
- When the speaker dictates numbered sections or subsections, preserve hierarchical numbering (e.g. Section 3(a)(ii))";

/// Seed the Legal builtin mode.
fn seed_legal_mode(db: &Database) -> AppResult<()> {
    let dict_entries: &[(&str, &str)] = &[
        // Latin phrases (commonly mangled by Whisper)
        ("habeas corpus", "habeas corpus"),
        ("prima fascia", "prima facie"),
        ("prima facia", "prima facie"),
        ("pro bono", "pro bono"),
        ("amicus curiae", "amicus curiae"),
        ("amicus curie", "amicus curiae"),
        ("certiorari", "certiorari"),
        ("stare decisis", "stare decisis"),
        ("starry decisis", "stare decisis"),
        ("res judicata", "res judicata"),
        ("res judiciata", "res judicata"),
        ("mens rea", "mens rea"),
        ("men's rea", "mens rea"),
        ("actus reus", "actus reus"),
        ("de facto", "de facto"),
        ("de jury", "de jure"),
        ("de jure", "de jure"),
        ("ex parte", "ex parte"),
        ("ex partay", "ex parte"),
        ("in camera", "in camera"),
        ("inter alia", "inter alia"),
        ("inter alya", "inter alia"),
        ("per curiam", "per curiam"),
        ("pro say", "pro se"),
        ("pro se", "pro se"),
        ("sua sponte", "sua sponte"),
        ("sub judice", "sub judice"),
        ("sub judy chay", "sub judice"),
        ("voir dire", "voir dire"),
        ("war dire", "voir dire"),
        ("nunc pro tunc", "nunc pro tunc"),
        ("force majeure", "force majeure"),
        ("force ma jure", "force majeure"),
        // Government agencies
        ("scotus", "SCOTUS"),
        ("d o j", "DOJ"),
        ("s e c", "SEC"),
        ("f t c", "FTC"),
        ("i r s", "IRS"),
        ("u s p t o", "USPTO"),
        ("f d a", "FDA"),
        ("e e o c", "EEOC"),
        ("o s h a", "OSHA"),
        ("n l r b", "NLRB"),
        // Court & procedure terms
        ("rule 12 b 6", "Rule 12(b)(6)"),
        ("rule 56", "Rule 56"),
        ("rule 26", "Rule 26"),
        ("fed r civ p", "Fed. R. Civ. P."),
        ("u s c", "U.S.C."),
        ("c f r", "C.F.R."),
        // Roles & titles
        ("j d", "J.D."),
        ("l l m", "LL.M."),
        ("esquire", "Esq."),
        // Common legal compound words
        ("counter claim", "counterclaim"),
        ("cross claim", "cross-claim"),
        ("here in after", "hereinafter"),
        ("here in", "herein"),
        ("there in", "therein"),
        ("there of", "thereof"),
        ("where as", "whereas"),
        ("where in", "wherein"),
        ("where of", "whereof"),
        ("afore mentioned", "aforementioned"),
        ("above mentioned", "above-mentioned"),
        ("non disclosure", "non-disclosure"),
        ("non compete", "non-compete"),
        // Common legal misspellings from speech
        ("plaintiff", "plaintiff"),
        ("plane tiff", "plaintiff"),
        ("defendant", "defendant"),
        ("deposition", "deposition"),
        ("subpoena", "subpoena"),
        ("sub peena", "subpoena"),
        ("affidavit", "affidavit"),
        ("affa david", "affidavit"),
        ("stipulation", "stipulation"),
        ("indemnification", "indemnification"),
        ("arbitration", "arbitration"),
        ("severability", "severability"),
    ];

    let snippet_entries: &[(&str, &str, &str)] = &[
        ("whereas clause", "WHEREAS, ", "Contract recital opener"),
        ("now therefore", "NOW, THEREFORE, in consideration of the mutual covenants and agreements set forth herein, the parties agree as follows:", "Contract transition clause"),
        ("respectfully submitted", "Respectfully submitted,", "Court filing closing"),
        ("to whom it may concern", "To Whom It May Concern:", "Formal letter opening"),
        ("please be advised", "Please be advised that", "Formal notice opener"),
        ("without prejudice", "Without prejudice to any rights or remedies,", "Reservation of rights clause"),
        ("confidentiality notice", "CONFIDENTIALITY NOTICE: This communication and any attachments are privileged and confidential, intended only for the use of the addressee. If you are not the intended recipient, please notify the sender immediately and delete this message.", "Email confidentiality footer"),
    ];

    seed_builtin_mode(
        db,
        BuiltinModeSeed {
            name: "Legal",
            description: "Optimized for legal writing and correspondence",
            icon: "scale",
            color: "purple",
            additions: LEGAL_ADDITIONS,
            sort_order: 4,
            writing_style: "formal",
            dictionary: dict_entries,
            snippets: snippet_entries,
        },
    )
}
