import { describe, expect, it } from 'vitest';
import ts from 'typescript';

const sources = import.meta.glob<string>(['../**/*.ts', '../**/*.svelte', '!../**/*.test.ts', '!../dev/**'], { query: '?raw', import: 'default', eager: true });

function imports(source: string, runtimeOnly = false): string[] {
  const scripts = source.includes('<script')
    ? Array.from(source.matchAll(/<script[^>]*>([\s\S]*?)<\/script>/g), match => match[1])
    : [source];
  return scripts.flatMap(script => {
    const ast = ts.createSourceFile('source.ts', script, ts.ScriptTarget.Latest, true);
    return ast.statements.flatMap(node => {
      if ((!ts.isImportDeclaration(node) && !ts.isExportDeclaration(node))
        || !node.moduleSpecifier || !ts.isStringLiteral(node.moduleSpecifier)) return [];
      if (runtimeOnly) {
        if (ts.isImportDeclaration(node)) {
          if (node.importClause?.isTypeOnly) return [];
          const bindings = node.importClause?.namedBindings;
          if (bindings && ts.isNamedImports(bindings) && !node.importClause?.name
            && bindings.elements.length > 0 && bindings.elements.every(binding => binding.isTypeOnly)) return [];
        } else {
          if (node.isTypeOnly) return [];
          const bindings = node.exportClause;
          if (bindings && ts.isNamedExports(bindings) && bindings.elements.length > 0
            && bindings.elements.every(binding => binding.isTypeOnly)) return [];
        }
      }
      return [node.moduleSpecifier.text];
    });
  });
}

describe('architecture boundaries', () => {
  it('keeps domain calculations independent of runtime and presentation', () => {
    for (const [path, source] of Object.entries(sources).filter(([path]) => path.startsWith('../domain/'))) {
      expect(imports(source).filter(specifier => /adapters|features|shells|\/ui\//.test(specifier)), path).toEqual([]);
      const ast = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true);
      const runtimeCalls: string[] = [];
      function visit(node: ts.Node): void {
        if (ts.isCallExpression(node) && /^(fetch|Date\.now|localStorage\.|indexedDB\.|window\.|document\.)/.test(node.expression.getText(ast))) runtimeCalls.push(node.expression.getText(ast));
        ts.forEachChild(node, visit);
      }
      visit(ast);
      expect(runtimeCalls, path).toEqual([]);
    }
  });

  it('keeps feature implementations on injected capabilities', () => {
    for (const [path, source] of Object.entries(sources).filter(([path]) => path.startsWith('../features/'))) {
      expect(imports(source).filter(specifier => specifier.includes('/adapters/')), path).toEqual([]);
    }
  });

  it('checks every production component instead of suppressing diagnostics', () => {
    for (const [path, source] of Object.entries(sources).filter(([path]) => path.endsWith('.svelte'))) {
      expect(source, path).not.toMatch(/@ts-nocheck/);
    }
  });

  it('keeps the hosted shell free of account capabilities', () => {
    const hosted = sources['../shells/HostedShell.svelte'];
    expect(hosted).toBeDefined();
    expect(imports(hosted).filter(specifier => /adapters\/(desktop|services)|ui\/desktop-context|features\/(inventory|selling|orders|watches)\//.test(specifier))).toEqual([]);
  });
});

function runtimeGraph(): Map<string, string[]> {
  const graph = new Map<string, string[]>();
  for (const [path, source] of Object.entries(sources)) {
    const dependencies: string[] = [];
    for (const specifier of imports(source, true)) {
      if (!specifier.startsWith('.')) continue;
      const target = '..' + new URL(specifier, new URL(path, 'https://source.invalid/src/')).pathname;
      const resolved = [target, target + '.ts', target + '.svelte', target.replace(/\.js$/, '.ts')].find(candidate => candidate in sources);
      if (resolved) dependencies.push(resolved);
    }
    graph.set(path, dependencies);
  }
  return graph;
}

it('keeps static production imports acyclic and hosted dependencies public', () => {
  const graph = runtimeGraph();
  const complete = new Set<string>();
  function visit(path: string, stack: string[]): void {
    expect(stack, `Import cycle: ${[...stack, path].join(' -> ')}`).not.toContain(path);
    if (complete.has(path)) return;
    for (const dependency of graph.get(path) ?? []) visit(dependency, [...stack, path]);
    complete.add(path);
  }
  for (const path of graph.keys()) visit(path, []);
  const reached = new Set<string>();
  function hosted(path: string): void {
    if (reached.has(path)) return;
    reached.add(path);
    expect(path).not.toMatch(/adapters\/(desktop|services)|ui\/desktop-context|features\/(inventory|selling|orders|watches)\//);
    for (const dependency of graph.get(path) ?? []) hosted(dependency);
  }
  hosted('../shells/HostedShell.svelte');
});

it('finds dependencies in both component scripts and re-exports', () => {
  const source = `<script context="module" lang="ts">export { service } from '../adapters/services';</script>
    <script lang="ts">import { view } from '../features/orders/view';</script>`;
  expect(imports(source)).toEqual(['../adapters/services', '../features/orders/view']);
});

it('keeps runtime side effects and value re-exports but omits type-only edges', () => {
  expect(imports(`
    import type { A } from './types-a';
    import { type B } from './types-b';
    export type * from './types-c';
    export { type D } from './types-d';
    import {} from './side-effect';
    import './initialize';
    export { value, type E } from './mixed';
    export * from './public';
  `, true)).toEqual(['./side-effect', './initialize', './mixed', './public']);
});
