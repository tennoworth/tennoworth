export function controllerTestRoot(setup: () => void): () => void {
  return $effect.root(setup);
}
