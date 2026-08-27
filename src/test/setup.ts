// Shared vitest setup. jsdom lacks a few layout-adjacent DOM APIs that the
// UI kit calls; stub them so components are testable without per-file shims.
if (typeof Element !== "undefined" && !Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
