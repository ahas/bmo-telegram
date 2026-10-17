// Bun reads a TOML file as a module; its shape is the reader's to state where it reads one.
declare module "*.toml" {
  const value: unknown;
  export default value;
}
