// B2 (RF refactoring-phase2): extract seedData.ts `export const initial*` arrays
// into canonical JSON files the Go engine serves via GET /api/collections/{name}.
// Pure data move (ponytail: scripted, not hand-transcribed 24KB). Run with node.
// Usage: node extract-seed.mjs <repoRoot>
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const repo = process.argv[2] || process.cwd();
const srcPath = join(repo, 'web-2', 'src', 'utils', 'seedData.ts');
const outDir = join(repo, 'web-2', 'data');
mkdirSync(outDir, { recursive: true });

const src = readFileSync(srcPath, 'utf8');

// Collection name mapping: initialShips -> ships.json (strip "initial", lowercase first).
const toFile = (name) => {
  const base = name.replace(/^initial/, '');
  // CamelCase -> kebab? keep simple: lowercase whole word (frontend uses /collections/ships etc.)
  return base.charAt(0).toLowerCase() + base.slice(1);
};

// Find each `export const NAME: Type[] = [ ... ];` block and eval the array literal.
// Strip the TS type annotation so plain JS eval works (object literals are valid JS).
const re = /export const (\w+)\s*:\s*[^=]+=\s*(\[[\s\S]*?\n\]);/g;
let m;
const written = [];
while ((m = re.exec(src)) !== null) {
  const name = m[1];
  const literal = m[2];
  let value;
  try {
    // eslint-disable-next-line no-eval
    value = (0, eval)('(' + literal + ')');
  } catch (err) {
    console.error(`SKIP ${name}: eval failed — ${err.message}`);
    continue;
  }
  const collection = toFile(name);
  const file = join(outDir, collection + '.json');
  writeFileSync(file, JSON.stringify(value, null, 2) + '\n', 'utf8');
  written.push(`${collection}.json (${Array.isArray(value) ? value.length : '?'} items)`);
}
console.log('WROTE:\n' + written.map((w) => '  ' + w).join('\n'));
console.log(`\nTotal: ${written.length} collections -> ${outDir}`);
