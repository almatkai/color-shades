// Executes original TypeScript services; only Raycast LocalStorage is intercepted.
const fs=require('node:fs'), path=require('node:path');
const {createRequire,registerHooks}=require('node:module');const {pathToFileURL}=require('node:url');
const root=path.resolve(__dirname,'..');const original=path.join(root,'.work/original');
const req=createRequire(path.join(original,'package.json'));
const mock=path.join(original,'mock-api.cjs');
fs.writeFileSync(mock,`const items={};exports.LocalStorage={setItem:async(k,v)=>{items[k]=v},allItems:async()=>({...items}),removeItem:async k=>{delete items[k]},clear:async()=>{for(const k in items)delete items[k]}};`);
registerHooks({resolve(s,c,n){return s==='@raycast/api'?{url:pathToFileURL(mock).href,shortCircuit:true}:n(s,c)}});
req('tsx/cjs');const {GeneratorService:G}=req('./src/services/generator.service.ts');const {PaletteService:P}=req('./src/services/palette.service.ts');const {StorageService:S}=req('./src/services/storage.service.ts');
const assert=require('node:assert/strict');const {camelCase}=req('change-case-all');
const RealDate=Date;const date='2026-01-02T03:04:05.678Z';global.Date=class extends RealDate {constructor(...args){super(...(args.length?args:[date]))}};
const inputs=['', '#', 'abc','#aBc','a#bc','1234','1','12','12345','123456','1234567','12345678','#123456FF','##abc','###abc','badcolor','red',' 12345','123456 ','123456\n','123456\r','123456\u2028','\n12','🐱a','a\n','####','ＦＦＦ','12#34','00000000','ffffff00'];
// Both arrays mutate via stable sort. The same tie follows both neighboring orders.
for(let i=0;i<10;i++)inputs.push('f00','ff0','0f0','0ff','00f','f0f','ff0','f00','0ff','0f0','f0f','00f');
let seed=0x1badb002;const rand=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed};
for(let i=0;i<2400;i++){const hex=(rand()&0xffffff).toString(16).padStart(6,'0');inputs.push(i%3===0?'#'+hex.toUpperCase():hex);if(i%8===0)inputs.push(hex.slice(0,3));}
for(let h=0;h<=360;h++){const c=req('color-convert').hsv.hex([h,100,100]);inputs.push(c);}
const ntc=req('color-namer/lib/colors/ntc');inputs.push(...ntc.map(n=>n.hex));
(async()=>{
const records=[];
for(const input of inputs){const before={cmy:[...G.CMY_HUES],rgb:[...G.RGB_HUES]};try{const palette=await G.generatePalette(input);records.push({input,before,palette,json:P.jsonStringPalette(palette),variables:P.variableDeclarationReadyPalette(palette)});}catch(e){records.push({input,before,error:String(e).toWellFormed()});}}
const data=JSON.parse(fs.readFileSync(path.join(root,'vendor/ntc.json')));assert.deepEqual(data,ntc.map(x=>({name:x.name,hex:x.hex,slug:camelCase(x.name)})));
await S.clearAllPalettes();await G.generatePalette('#abc');await G.generatePalette('#aBc');let all=await S.allPalettes();assert.equal(all.length,1);assert.equal(all[0].colors[500],'#aaBBcc');assert.ok(all[0].creationDate instanceof Date);
await G.generatePalette('#ff0000');all=await S.allPalettes();assert.equal(all.length,2);await S.removePalette(all[0]);assert.equal((await S.allPalettes()).length,1);await S.clearAllPalettes();assert.equal((await S.allPalettes()).length,0);
fs.writeFileSync(path.join(root,'fixtures/oracle.json'),JSON.stringify({date,records})+'\n');console.log(`Original TS oracle: ${records.length} sequential inputs, ${records.filter(x=>x.error).length} errors; all ${ntc.length} slugs and original storage exercised`);
})().catch(e=>{console.error(e);process.exitCode=1});
