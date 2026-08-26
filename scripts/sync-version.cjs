const fs = require('fs');
const path = require('path');

const ver = process.argv[2];
if (!ver) { console.error('[ERROR] version required'); process.exit(1); }

function updateJson(file, version) {
  const full = path.resolve(file);
  const content = fs.readFileSync(full, 'utf8');
  const updated = content.replace(/"version"\s*:\s*"[^"]*"/, `"version": "${version}"`);
  fs.writeFileSync(full, updated, 'utf8');
  console.log(`  ${file} -> ${version}`);
}

function updateCargoToml(file, version) {
  const full = path.resolve(file);
  const content = fs.readFileSync(full, 'utf8');
  const updated = content.replace(/^(version\s*=\s*)"[^"]*"/m, `$1"${version}"`);
  fs.writeFileSync(full, updated, 'utf8');
  console.log(`  ${file} -> ${version}`);
}

updateJson('package.json', ver);
updateJson('src-tauri/tauri.conf.json', ver);
updateCargoToml('src-tauri/Cargo.toml', ver);

console.log('[SChat] Version synced.');
