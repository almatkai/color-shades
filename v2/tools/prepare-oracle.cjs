// Reconstruct an isolated executable copy from the pinned files packaged here.
const fs=require('node:fs'),path=require('node:path');const root=path.resolve(__dirname,'..');
const source=path.join(root,'fixtures/original'),dest=path.join(root,'.work/original');fs.mkdirSync(dest,{recursive:true});
fs.cpSync(path.join(source,'src'),path.join(dest,'src'),{recursive:true});
for(const [from,to] of [['own-package.json','package.json'],['package-lock.json','package-lock.json']])fs.copyFileSync(path.join(source,from),path.join(dest,to));
console.log('Prepared v2/.work/original from vendored pinned TS and own locked dependencies. Run npm ci --ignore-scripts there, then node tools/oracle.cjs from v2.');
