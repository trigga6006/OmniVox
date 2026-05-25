const BAR_COUNT = 5;

export function IdleWaveform({ color }: { color: string }) {
  return (
    <div className="flex items-center justify-center gap-[3px] w-full h-full">
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <div
          key={i}
          className="rounded-full"
          style={{
            width: 2,
            height: 10,
            backgroundColor: color,
            opacity: 0.25,
            willChange: "transform, opacity",
            animation: `idle-wave 2.4s ease-in-out ${i * 0.18}s infinite`,
          }}
        />
      ))}
    </div>
  );
}
