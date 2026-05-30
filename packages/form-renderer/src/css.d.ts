// The shared form stylesheet is imported as a raw string (`?inline`) so it can
// be injected into a shadow root via a constructable stylesheet. form-renderer
// is consumed as source by Vite-based bundlers (admin, embed SDK); this ambient
// declaration keeps `tsc --noEmit` happy without pulling in `vite/client`.
declare module "*.css?inline" {
  const css: string;
  export default css;
}
