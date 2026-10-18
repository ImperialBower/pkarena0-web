#!/usr/bin/env node
// The embed release, as one flat directory (EPIC-56 Phase 4).
//
//   node scripts/embed-bundle.mjs <out-dir> [--tag vX.Y.Z] [--max-wasm BYTES]
//
// Writes:  pk-table.js  pk-table.css  pkarena0_web.js  pkarena0_web_bg.wasm
//          LICENSE-MIT  LICENSE-APACHE  MANIFEST.sha256
//
// pk-table.js is www/js/embed/pk-table.js with its three relative imports —
// and table.js's one — inlined, by string work, not a bundler: each module
// becomes a block that returns its exports, and every `import { a } from
// './x.js'` becomes `const { a } = __m_x;`. Nothing else is rewritten, so
// the `?instance=` import that keeps two engines apart (rule 2) survives;
// a real bundler might normalise that query away, which is why this isn't one.
//
// `--tag` asserts the tag names Cargo.toml's version — the badge shows
// env!("CARGO_PKG_VERSION"), so a mismatch would be a lie on the page.
// `--max-wasm` (default 2,300,000) is the hup-charts tripwire: that feature
// once took the download from 478 KB to 3.84 MB by accident.
//
// Fails before writing anything; a failed run leaves no out-dir behind.
import { readFileSync, writeFileSync, mkdirSync, existsSync, rmSync, copyFileSync } from 'fs';
import { createHash } from 'crypto';
import path from 'path';
import { fileURLToPath } from 'url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const out = args[0] && !args[0].startsWith('--') ? path.resolve(args[0]) : null;
const opt = (name, dflt) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : dflt; };
const tag = opt('--tag', null);
const maxWasm = Number(opt('--max-wasm', 2_300_000));

function die(msg) { console.error('embed-bundle: ' + msg); process.exit(1); }
if (!out) die('usage: embed-bundle.mjs <out-dir> [--tag vX.Y.Z] [--max-wasm BYTES]');
if (existsSync(out)) die(`${out} exists; remove it first`);

const read = rel => readFileSync(path.join(ROOT, rel), 'utf8');

// ── Checks, all before any write ─────────────────────────────────────────────
const version = /^version\s*=\s*"([^"]+)"/m.exec(read('Cargo.toml'))?.[1];
if (!version) die('no version in Cargo.toml');
if (tag !== null && tag.replace(/^v/, '') !== version) {
  die(`tag ${tag} is not the crate version ${version}; bump Cargo.toml or tag v${version}`);
}
for (const f of ['www/pkg/pkarena0_web.js', 'www/pkg/pkarena0_web_bg.wasm']) {
  if (!existsSync(path.join(ROOT, f))) die(`${f} is missing; run wasm-pack build first`);
}
const wasmBytes = readFileSync(path.join(ROOT, 'www/pkg/pkarena0_web_bg.wasm')).length;
if (wasmBytes >= maxWasm) die(`pkarena0_web_bg.wasm is ${wasmBytes} bytes, over the ${maxWasm} gate (hup-charts on?)`);
// The committed stylesheet carries table.css verbatim; refuse to ship a stale copy.
const css = read('www/css/pk-table.css');
const tableCss = read('www/css/table.css');
if (!css.endsWith(tableCss)) die('www/css/pk-table.css no longer ends with www/css/table.css; regenerate it');
if (/@import|url\(/.test(css)) die('pk-table.css references something outside itself');

// ── The module ───────────────────────────────────────────────────────────────
// Order is dependency order; each entry names what the others may import.
const modules = [
  { id: 'cards',   file: 'www/js/cards.js' },
  { id: 'table',   file: 'www/js/table.js' },
  { id: 'actions', file: 'www/js/actions.js' },
  { id: 'embed',   file: 'www/js/embed/pk-table.js' },
];
const byBasename = Object.fromEntries(modules.map(m => [path.basename(m.file), m.id]));

function inline(m) {
  let src = read(m.file);
  const exports = [];
  // import { a, b } from './x.js';  →  const { a, b } = __m_x;
  src = src.replace(/^\s*import\s*\{([^}]*)\}\s*from\s*['"]([^'"]+)['"];?\s*$/gm, (_, names, from) => {
    const id = byBasename[path.basename(from)];
    if (!id) die(`${m.file} imports ${from}, which the bundle does not carry`);
    return `const {${names}} = __m_${id};`;
  });
  if (/^\s*import\s/m.test(src)) die(`${m.file} has an import the bundler does not understand`);
  src = src.replace(/^export\s+(function|class|const|let)\s+([A-Za-z_$][\w$]*)/gm, (_, kind, name) => {
    exports.push(name);
    return `${kind} ${name}`;
  });
  if (/^export\s/m.test(src)) die(`${m.file} has an export the bundler does not understand`);
  return `// ── ${m.file} ──\nconst __m_${m.id} = (() => {\n${src}\nreturn { ${exports.join(', ')} };\n})();\n`;
}

const banner = `// pk-table.js — pkarena0-web v${version}, the embed build (EPIC-56).
// One file: www/js/cards.js, table.js, actions.js and embed/pk-table.js,
// inlined by scripts/embed-bundle.mjs. Source and license:
// https://github.com/ImperialBower/pkarena0-web/tree/v${version}
`;
const js = banner + modules.map(inline).join('\n') + `\nexport const { PkTable } = __m_embed;\n`;

// ── Write, then the manifest over what was written ───────────────────────────
mkdirSync(out, { recursive: true });
writeFileSync(path.join(out, 'pk-table.js'), js);
writeFileSync(path.join(out, 'pk-table.css'), css);
copyFileSync(path.join(ROOT, 'www/pkg/pkarena0_web.js'), path.join(out, 'pkarena0_web.js'));
copyFileSync(path.join(ROOT, 'www/pkg/pkarena0_web_bg.wasm'), path.join(out, 'pkarena0_web_bg.wasm'));
copyFileSync(path.join(ROOT, 'LICENSE-MIT'), path.join(out, 'LICENSE-MIT'));
copyFileSync(path.join(ROOT, 'LICENSE-APACHE'), path.join(out, 'LICENSE-APACHE'));
const files = ['LICENSE-APACHE', 'LICENSE-MIT', 'pk-table.css', 'pk-table.js', 'pkarena0_web.js', 'pkarena0_web_bg.wasm'];
const manifest = files
  .map(f => createHash('sha256').update(readFileSync(path.join(out, f))).digest('hex') + '  ' + f)
  .join('\n') + '\n';
writeFileSync(path.join(out, 'MANIFEST.sha256'), manifest);
console.log(`embed-bundle: v${version} → ${out} (wasm ${wasmBytes} bytes, pk-table.js ${js.length} bytes)`);
