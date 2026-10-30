export function controllerTestRoot(setup: () => void): () => void {
  return $effect.root(setup);
}

/** One reactive value for a test to drive a controller input with. */
export function reactiveBox<T>(initial: T): { value: T } {
  let value = $state(initial);
  return { get value() { return value; }, set value(next: T) { value = next; } };
}
