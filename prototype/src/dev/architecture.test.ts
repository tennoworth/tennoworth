import { describe, expect, it } from 'vitest';
import ts from 'typescript';

const sources = import.meta.glob<string>(['../**/*.ts', '../**/*.svelte', '!../**/*.test.ts', '!../dev/**'], { query: '?raw', import: 'default', eager: true });

function imports(source: string): string[] {
  const script = source.includes('<script') ? source.match(/<script[^>]*>([\s\S]*?)<\/script>/)?.[1] ?? '' : source;
  const ast = ts.createSourceFile('source.ts', script, ts.ScriptTarget.Latest, true);
  return ast.statements.flatMap(node => ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier) ? [node.moduleSpecifier.text] : []);
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
