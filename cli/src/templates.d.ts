/** Template files are inlined into the CLI binary at compile time via Bun's
 *  text loader, so an installed CLI has no asset directory to lose. */
declare module "*.tmpl" {
  const contents: string;
  export default contents;
}
