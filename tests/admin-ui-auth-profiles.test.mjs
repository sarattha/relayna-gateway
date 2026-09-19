import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const source=readFileSync(new URL('../crates/gateway-api/admin-ui/src/auth-profiles.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ES2022,target:ts.ScriptTarget.ES2022}}).outputText;
const {profileRow,profileFields,profilesFromForm,showProfileError}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const id='00000000-0000-0000-0000-000000000001';
const form=new FormData();
form.set('profiles_revision','4');form.append('profile_slot','employee');form.append('profile_slot','automation');
for(const [key,value] of Object.entries({'employee.id':'employees','employee.name':'Employees','employee.type':'entra_and_relayna_key','employee.enabled':'on','employee.audience':'api://employees','employee.required_scopes':'run, read','employee.required_roles':'invoke','employee.allowed_groups':'staff','employee.keys':id,'automation.id':'automation','automation.name':'Automation','automation.type':'relayna_key_only','automation.enabled':'on'}))form.set(key,value);
let result=profilesFromForm(form);
assert.equal(result.revision,4);assert.equal(result.profiles.length,2);
assert.deepEqual(result.profiles[0].entra.required_scopes,['run','read']);
assert.deepEqual(result.bindings,[{key_id:id,profile_id:'employees'}]);
assert.equal(result.profiles[1].entra,undefined);
assert.throws(()=>profilesFromForm(form,true),/Accessa/);
form.set('automation.keys',id);assert.throws(()=>profilesFromForm(form),/only once/);form.delete('automation.keys');
form.set('automation.id','employees');assert.throws(()=>profilesFromForm(form),/unique/);form.set('automation.id','automation');
form.set('employee.audience','');assert.throws(()=>profilesFromForm(form),/audience/);form.set('employee.audience','api://employees');
form.delete('employee.enabled');assert.equal(profilesFromForm(form).profiles[0].enabled,false);
form.set('employee.required_roles',Array(65).fill('role').join(','));assert.throws(()=>profilesFromForm(form),/at most 64/);
assert.throws(()=>profilesFromForm(new FormData()),/1–32/);
assert.match(profileFields({authentication_profiles:result}),/4/);
const hostile=profileRow({id:'<img>',name:'"<script>',entra:{audience:'<svg>'}},[]);
assert.doesNotMatch(hostile,/<img>|<script>|<svg>/);assert.match(hostile,/&lt;script&gt;/);
console.log('ok - authentication profile forms validate bindings, independent claims, Accessa restrictions, revisions and escaped values');

assert.match(profileFields({}), /panel-heading[^]*data-add-profile>Add profile/);
assert.equal((profileFields({}).match(/data-add-profile/g)||[]).length,1);
const main=readFileSync(new URL('../crates/gateway-api/admin-ui/src/main.ts',import.meta.url),'utf8');
assert.match(main, /<form class="modal-form"><div class="modal-scroll form-grid">\$\{endpointIdentityFields\(setting.access\)\}<\/div>/);

const pattern = profileRow().match(/pattern="([^"]+)"/)[1];
const browserPattern = new RegExp(`^(?:${pattern})$`, 'v');
assert.equal(browserPattern.test('employee-app_1'), true);
assert.equal(browserPattern.test('employee app'), false);
const notice = {attributes:{}, setAttribute(key,value){this.attributes[key]=value;}, focus(){this.focused=true;}, scrollIntoView(){this.scrolled=true;}};
assert.equal(showProfileError({querySelector:()=>notice}, 'Each Entra profile requires a valid audience.'),true);
assert.equal(notice.textContent,'Each Entra profile requires a valid audience.');
assert.equal(notice.attributes.role,'alert');assert.equal(notice.focused,true);assert.equal(notice.scrolled,true);
assert.equal(showProfileError({querySelector:()=>null},'error'),false);

const {syncIdentityFields,bindProfileEditor}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
function control(name,value='',type='text') {return {name,value,type,disabled:false,required:false,checked:true};}
function group(controls,extra={}) {return {hidden:false,querySelectorAll:()=>controls,querySelector:selector=>extra[selector],...extra};}
const mode=control('endpoint_entra_mode','profiles');
const endpointAudience=control('endpoint_audience','api://legacy');
const endpoint=group([endpointAudience],{'[name="endpoint_audience"]':endpointAudience});
const profileControls=[control('profile_slot','test'),control('test.id','automation'),control('test.name','Automation'),control('test.type','entra_and_relayna_key'),control('test.enabled','on','checkbox'),control('test.audience','api://employees'),control('test.required_scopes','invoke'),control('test.keys',id)];
const entraGroup=group(profileControls.slice(5,7));
const keyHint={hidden:true};
const row={querySelector:selector=>({'[data-profile-type]':profileControls[3],'[data-profile-entra]':entraGroup,'[name$=".audience"]':profileControls[5],'[data-profile-key-only]':keyHint}[selector])};
const editor={hidden:false,querySelectorAll:selector=>selector==='[data-profile-row]'?[row]:profileControls};
const root={querySelector:selector=>({'[name="endpoint_entra_mode"]':mode,'[data-endpoint-entra]':endpoint,'[data-profile-editor]':editor}[selector])};
const handlers={};bindProfileEditor({addEventListener:(event,handler)=>{handlers[event]=handler;}});
const change=target=>handlers.change({target:{matches:()=>true,closest:()=>root,...target}});
const submitted=()=>{const data=new FormData();for(const c of profileControls)if(!c.disabled&&(c.type!=='checkbox'||c.checked))data.append(c.name,c.value);return data;};
syncIdentityFields(root);
assert.equal(endpoint.hidden,true);assert.equal(endpointAudience.disabled,true);
assert.equal(editor.hidden,false);assert.equal(entraGroup.hidden,false);assert.equal(profileControls[5].required,true);
assert.equal(profilesFromForm(submitted()).profiles[0].entra.audience,'api://employees');
profileControls[3].value='relayna_key_only';change(profileControls[3]);
assert.equal(entraGroup.hidden,true);assert.equal(profileControls[5].disabled,true);assert.equal(profileControls[5].required,false);assert.equal(keyHint.hidden,false);
assert.equal(submitted().has('test.audience'),false);assert.equal(profilesFromForm(submitted()).profiles[0].entra,undefined);
profileControls[3].value='entra_and_relayna_key';change(profileControls[3]);
assert.equal(profileControls[5].value,'api://employees','switching back retains the unsaved audience');
assert.deepEqual(profilesFromForm(submitted()).profiles[0].entra.required_scopes,['invoke']);
for(const selected of ['inherit','disabled','required']) {
  mode.value=selected;change(mode);
  assert.equal(editor.hidden,true);assert.equal(profileControls.every(c=>c.disabled),true);
  assert.equal(endpoint.hidden,selected!=='required');assert.equal(endpointAudience.required,selected==='required');
  assert.equal(submitted().has('profile_slot'),false,'inactive profile drafts do not submit');
}
mode.value='profiles';change(mode);assert.equal(profileControls[1].value,'automation');assert.equal(profileControls[1].disabled,false);
assert.match(profileRow({type:'relayna_key_only'}), /data-profile-entra hidden/);
assert.match(profileRow({type:'relayna_key_only'}), /input disabled name="profile-\d+\.audience"/);
assert.match(profileRow(), /input required name="profile-\d+\.audience"/);
console.log('ok - dynamic identity modes hide and exclude inactive controls, require Entra audience only, and preserve drafts');

const {profileKeyOptions,selectedKeyIds,loadProfileKeys,refreshProfileKeys}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const secondId='00000000-0000-0000-0000-000000000002';
const inventory={keys:[{id,key_prefix:'rk_employee',project_id:'project-one',service_names:['responses']},{id:secondId,key_prefix:'rk_robot',project_id:'project-two',disabled:true,service_names:['embeddings']}],projects:[{id:'project-one',name:'Research'},{id:'project-two',name:'Automation'}]};
for(const query of ['RK_EMPLOYEE',id,'research','PROJECT-ONE','responses','research employee'])assert.deepEqual(profileKeyOptions(inventory.keys,inventory.projects,query).map(k=>k.id),[id]);
assert.equal(profileKeyOptions(inventory.keys,inventory.projects,'missing').length,0);
assert.deepEqual(profileKeyOptions(inventory.keys,inventory.projects,'','project-two').map(k=>k.id),[secondId]);
assert.equal(profileKeyOptions(inventory.keys,inventory.projects,'robot')[0].status,'Disabled');
assert.deepEqual(selectedKeyIds(' ABC ,\n DEF '),['abc','def']);
assert.doesNotMatch(profileRow(),/<textarea/);
assert.match(profileRow(),/type="search" data-key-search/);
const pickerHandlers={};
const makePicker=value=>{
  const nodes={
    '[data-profile-keys]':{value},'[data-selected-keys]':{innerHTML:''},
    '[data-key-search]':{value:'',focus(){this.focused=true;}},
    '[data-key-search-panel]':{hidden:true},'[data-key-results]':{innerHTML:''},
    '[data-key-results-status]':{textContent:''},'[data-open-keys]':{focus(){},setAttribute(){}}
  };
  return {nodes,querySelector:selector=>nodes[selector]};
};
const firstPicker=makePicker(id),secondPicker=makePicker('');
const pickerRoot={dataset:{keyProject:''},querySelector:()=>({value:'profiles'})};
const pickerEditor={isConnected:true,closest:()=>pickerRoot,querySelectorAll:selector=>selector==='[data-profile-keys]'?[firstPicker.nodes[selector],secondPicker.nodes[selector]]:[firstPicker,secondPicker]};
bindProfileEditor({addEventListener:(type,fn)=>pickerHandlers[type]=fn},async()=>inventory);
const clickPicker=(picker,action,key)=>pickerHandlers.click({target:{closest:selector=>selector==='[data-profile-editor]'?pickerEditor:selector==='[data-key-picker]'?picker:selector===action?{dataset:{addKey:key,removeKey:key},setAttribute(){}}:null}});
await loadProfileKeys(pickerEditor,async()=>inventory);
assert.match(firstPicker.nodes['[data-selected-keys]'].innerHTML,/rk_employee.*Research/s);
assert.match(secondPicker.nodes['[data-key-results]'].innerHTML,/Assigned elsewhere/);
await clickPicker(secondPicker,'[data-add-key]',id);
assert.equal(secondPicker.nodes['[data-profile-keys]'].value,'','cannot assign a key already bound to another profile');
await clickPicker(secondPicker,'[data-add-key]',secondId);
assert.equal(secondPicker.nodes['[data-profile-keys]'].value,secondId);
await clickPicker(firstPicker,'[data-remove-key]',id);
assert.equal(firstPicker.nodes['[data-profile-keys]'].value,'');
await clickPicker(secondPicker,'[data-add-key]',id);
assert.equal(secondPicker.nodes['[data-profile-keys]'].value,`${secondId},${id}`);
await clickPicker(secondPicker,'[data-remove-key]',secondId);
pickerRoot.dataset.keyProject='project-one';
await clickPicker(firstPicker,'[data-add-key]',secondId);
assert.equal(firstPicker.nodes['[data-profile-keys]'].value,'','unassigned keys from another project cannot be added');
pickerRoot.dataset.keyProject='';
await clickPicker(secondPicker,'[data-add-key]',secondId);
secondPicker.nodes['[data-profile-keys]'].value=[secondId,id].join(',');
pickerRoot.dataset.keyProject='project-one';
secondPicker.nodes['[data-key-search]'].value='missing';refreshProfileKeys(pickerEditor);
assert.equal(secondPicker.nodes['[data-key-results]'].innerHTML,'');
assert.match(secondPicker.nodes['[data-selected-keys]'].innerHTML,/rk_robot/,'search never drops selected keys');
await loadProfileKeys(pickerEditor,async()=>{throw new Error('offline')});
assert.match(secondPicker.nodes['[data-key-results-status]'].textContent,/Existing selections are kept/);
assert.equal(secondPicker.nodes['[data-profile-keys]'].value,`${secondId},${id}`);
await loadProfileKeys(pickerEditor,async()=>inventory);
assert.doesNotMatch(secondPicker.nodes['[data-key-results-status]'].textContent,/Could not load/);
const unknown='00000000-0000-0000-0000-000000000099';firstPicker.nodes['[data-profile-keys]'].value=unknown;
await loadProfileKeys(pickerEditor,async()=>({keys:[{id:secondId,key_prefix:'<script>"',project_id:'project-two'}],projects:inventory.projects}));
assert.match(firstPicker.nodes['[data-selected-keys]'].innerHTML,/Key details unavailable/);
assert.equal(firstPicker.nodes['[data-profile-keys]'].value,unknown);
assert.doesNotMatch(secondPicker.nodes['[data-selected-keys]'].innerHTML,/<script>/);
assert.match(secondPicker.nodes['[data-selected-keys]'].innerHTML,/&lt;script&gt;/);
console.log('ok - key picker search, project scope, duplicate prevention, explicit removal, failure/retry, preserved unknown selections and escaped labels');

let preventedSearchSubmit=false;
pickerHandlers.keydown({key:'Enter',target:{matches:()=>true},preventDefault(){preventedSearchSubmit=true;}});
assert.equal(preventedSearchSubmit,true,'Enter in key search must not submit the identity form');
