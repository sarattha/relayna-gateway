import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const source = readFileSync(new URL('../crates/gateway-api/admin-ui/src/main.ts',import.meta.url),'utf8');
const mountSource = source.slice(source.indexOf('function mountDialog('),source.indexOf('\nfunction closeTopDialog('));
const roots = [];
let focused = null;
class Element {
  constructor({inert=false,modal=false,dialog=null}={}) { this.inert=inert;this.modal=modal;this.dialog=dialog;this.isConnected=true; }
  matches() { return this.modal; }
  querySelector() { return this.dialog; }
  addEventListener() {}
  removeEventListener() {}
  remove() {this.isConnected=false;roots.splice(roots.indexOf(this),1);}
  focus() {focused=this;}
}
const enabledRoot = new Element();
const preInertRoot = new Element({inert:true});
const originalFocus = new Element();
roots.push(enabledRoot,preInertRoot);
const document = {body:{children:roots},activeElement:originalFocus,querySelectorAll:()=>roots.filter(root=>root.modal)};
const mount = new Function('document','HTMLElement','dialogInertRoots','queueMicrotask',`${mountSource}; return mountDialog;`)(document,Element,new Map(),()=>{});
function modal() {const node=new Element({modal:true,dialog:new Element()});roots.push(node);return node;}
const outer=modal();const closeOuter=mount(outer);
const inner=modal();const closeInner=mount(inner);
assert.equal(enabledRoot.inert,true);
assert.equal(outer.inert,true);
// An outer workflow can finish while an inner confirmation is still mounted.
closeOuter();
assert.equal(enabledRoot.inert,true);
closeInner();
assert.equal(enabledRoot.inert,false);
assert.equal(preInertRoot.inert,true);
let replacement = originalFocus;
const closeLive = mount(modal(),{restoreFocus:()=>replacement});
originalFocus.isConnected=false;
replacement=new Element();
closeLive();
assert.equal(focused,replacement);
assert.equal(enabledRoot.inert,false);
assert.equal(preInertRoot.inert,true);
console.log('ok - dialogs preserve root inert baselines across nested close order and resolve current focus targets');
