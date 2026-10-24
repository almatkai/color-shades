// Run byte-exact beta parser and rendering code. Only unrelated imported UI
// components are stubbed; React rendering, swatches, parsers and back routing are real.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {createRequire,registerHooks}=require('node:module');const {pathToFileURL}=require('node:url');
const root=path.resolve(__dirname,'..'),base=path.join(root,'.work/original'),req=createRequire(path.join(root,'tools/package.json'));
const stub=path.join(base,'renderer-stub.cjs');
fs.writeFileSync(stub,`const React=require('react');const C=p=>React.createElement('span',null,p.children);exports.Hint=exports.HintBar=exports.Kbd=exports.ViewHeader=exports.Markdown=exports.MetadataItem=exports.MetadataSidebar=C;exports.cx=(...v)=>v.filter(Boolean).join(' ');exports.buildColorConversionResults=()=>[];exports.parseNativeColorWheel=()=>null;exports.NativeColorWheel=C;exports.Action={};exports.ActionPanel=C;exports.ActionPanelOverlay=C;exports.ActionRegistryContext=React.createContext({actions:[]});exports.NavigationContext=React.createContext({});exports.setRuntimeContext=()=>{};`);
const reactPaths=Object.fromEntries(['react','react/jsx-runtime'].map(s=>[s,pathToFileURL(req.resolve(s)).href]));
registerHooks({resolve(s,c,n){if(c.parentURL?.includes('/fixtures/beta/')&&(/ui\/|colorConverter|detail-runtime|hooks|action-runtime/.test(s)))return {url:pathToFileURL(stub).href,shortCircuit:true};if(s==='react'||s==='react/jsx-runtime')return {url:reactPaths[s],shortCircuit:true};return n(s,c)}});
req('tsx/cjs');const React=req('react'),{renderToStaticMarkup}=req('react-dom/server');
// The byte-exact fixtures use production JSX compilation's automatic runtime.
global.React=React;
const {GridRuntime}=require('../fixtures/beta/grid-runtime.tsx');const {colorFromGridContent}=require('../fixtures/beta/grid-content.ts');
const {ListRuntime}=require('../fixtures/beta/list-runtime.tsx');const {FormRuntime}=require('../fixtures/beta/form-runtime.tsx');const {backActionId}=require('../fixtures/beta/index.tsx');
const wire=JSON.parse(fs.readFileSync(path.join(root,'fixtures/wire.json')));const noop=()=>{};
const props=v=>({root:v.root,title:v.title,actions:v.actions,onBack:noop,onRunPrimaryAction:noop,onOpenActions:noop,onSearchTextChanged:noop,onLoadMore:noop,onSubmitForm:noop,onChangeField:noop,onInvokeAction:noop});
const grid=renderToStaticMarkup(React.createElement(GridRuntime,props(wire.grid)));assert.match(grid,/repeat\(8, minmax\(0, 1fr\)\)/);
for(const row of wire.grid.root.children){const hex=row.props.subtitle;assert.equal(colorFromGridContent(row.props.content),hex);assert.ok(grid.includes(hex));assert.ok(grid.includes('background-color:'+hex));assert.ok(grid.includes('>'+row.props.title+'</p>'));assert.equal(row.props.actionIds.length,5);for(const id of row.props.actionIds)assert.ok(wire.grid.actions.some(a=>a.id===id));}
assert.equal(backActionId(wire.grid.root),wire.grid.actions.at(-1).id);
const list=renderToStaticMarkup(React.createElement(ListRuntime,props(wire.list)));for(const row of wire.list.root.children){assert.ok(list.includes(row.props.title));assert.ok(list.includes('background-color:'+row.props.icon.tintColor));assert.ok(list.includes(new Date(row.props.accessories[0].date).toLocaleDateString()));}
const form=renderToStaticMarkup(React.createElement(FormRuntime,props(wire.form)));assert.ok(form.includes('#82123d'));assert.ok(form.includes('Color'));assert.equal(wire.form.actions[0].kind,'submit-form');
// Execute the actual (non-exported) parser helpers too, without rewriting them.
const ts=req('typescript');const vm=require('node:vm');
function parser(file,exportNames){const source=fs.readFileSync(path.join(root,'fixtures/beta',file),'utf8')+'\n'+exportNames.map(n=>`export {${n}};`).join('\n');const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.React}}).outputText;const module={exports:{}};const localRequire=s=>s==='react'?React:s==='./grid-content'?require('../fixtures/beta/grid-content.ts'):req(stub);vm.runInNewContext(js,{module,exports:module.exports,require:localRequire,React,Date,console});return module.exports;}
const items=parser('grid-runtime.tsx',['collectGridItems']).collectGridItems(wire.grid.root);assert.equal(items.length,11);assert.deepEqual([...items].map(x=>x.color),wire.grid.root.children.map(x=>x.props.subtitle));
const rows=parser('list-runtime.tsx',['parseListRows']).parseListRows(wire.list.root);assert.equal(rows.length,wire.list.root.children.length);assert.equal(rows[0].icon.source,'CircleFilled');
const fields=parser('form-runtime.tsx',['collectFormItems']).collectFormItems(wire.form.root);assert.equal(fields[0].field.id,'color');
console.log('Actual beta Grid/List/Form parsers, swatch SSR, date accessory and back routing: PASS (8-column clamp confirmed)');
