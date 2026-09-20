import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const source=readFileSync(new URL('../crates/gateway-api/admin-ui/src/auth-profiles.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ES2022,target:ts.ScriptTarget.ES2022}}).outputText;
const {profileRow,profileFields,profilesFromForm,showProfileError,generateProfileId}=await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
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
form.set('automation.keys',id);assert.throws(()=>profilesFromForm(form),/assigned more than once/);form.delete('automation.keys');
form.set('automation.id','employees');assert.throws(()=>profilesFromForm(form),/already used/);form.set('automation.id','automation');
form.set('employee.audience','');assert.throws(()=>profilesFromForm(form),/audience/);form.set('employee.audience','api://employees');
form.delete('employee.enabled');assert.equal(profilesFromForm(form).profiles[0].enabled,false);
form.set('employee.required_roles',Array(65).fill('role').join(','));assert.throws(()=>profilesFromForm(form),/at most 64/);
assert.throws(()=>profilesFromForm(new FormData()),/^Error: Add at least one authentication profile before saving\.$/);
const limitForm = new FormData();
for (let index = 0; index < 33; index++) {
  const slot = `limit-${index}`;
  limitForm.append('profile_slot', slot);
  limitForm.set(`${slot}.id`, slot);
  limitForm.set(`${slot}.name`, slot);
  limitForm.set(`${slot}.type`, 'relayna_key_only');
}
assert.throws(()=>profilesFromForm(limitForm), /more than 32 authentication profiles/);
limitForm.delete('profile_slot');
limitForm.append('profile_slot', 'limit-0');
limitForm.set('limit-0.keys', Array.from({length:1025}, (_, index) => `00000000-0000-0000-0000-${String(index).padStart(12,'0')}`).join(','));
assert.throws(()=>profilesFromForm(limitForm), /more than 1,024 assigned keys/);
assert.match(profileFields({authentication_profiles:result}),/4/);
const hostile=profileRow({id:'<img>',name:'"<script>',entra:{audience:'<svg>'}},[]);
assert.doesNotMatch(hostile,/<img>|<script>|<svg>/);assert.match(hostile,/&lt;script&gt;/);
const generatedForm = new FormData();
generatedForm.append('profile_slot','new');
generatedForm.set('new.name','Pródüction Worker / 2026');
generatedForm.set('new.type','relayna_key_only');
generatedForm.set('new.keys',id);
const generated = profilesFromForm(generatedForm);
assert.match(generated.profiles[0].id,/^production-worker-2026-[a-f0-9]{8}$/);
assert.equal(generated.bindings[0].profile_id,generated.profiles[0].id);
assert.notEqual(profilesFromForm(generatedForm).profiles[0].id,generated.profiles[0].id);
for (const name of ['a'.repeat(120),'ทีมงาน','---','0']) {
  const autoId = generateProfileId(name,new Set());
  assert.match(autoId,/^[a-z0-9-]{1,64}$/);
  if (name === 'ทีมงาน' || name === '---') assert.match(autoId,/^profile-/);
}
generatedForm.set('new.original_id',generated.profiles[0].id);
generatedForm.set('new.name','Renamed worker');
assert.equal(profilesFromForm(generatedForm).profiles[0].id,generated.profiles[0].id);
generatedForm.set('new.id','Custom_ID');
assert.equal(profilesFromForm(generatedForm).profiles[0].id,'Custom_ID');
assert.equal(profilesFromForm(generatedForm).bindings[0].profile_id,'Custom_ID');
generatedForm.set('new.id','invalid id');
assert.throws(()=>profilesFromForm(generatedForm),/Stable profile ID must use/);
const getRandomValues = crypto.getRandomValues;
try {
  let calls = 0;
  crypto.getRandomValues = values => { values[0] = ++calls < 3 ? 0xaaaaaaaa : 0xbbbbbbbb; return values; };
  const collisionForm = new FormData();
  for (const slot of ['auto','manual']) {
    collisionForm.append('profile_slot',slot);
    collisionForm.set(`${slot}.name`,slot === 'auto' ? 'Worker' : 'Manual worker');
    collisionForm.set(`${slot}.type`,'relayna_key_only');
  }
  collisionForm.set('manual.id','worker-aaaaaaaa');
  const resolved = profilesFromForm(collisionForm);
  assert.equal(calls,3,'retry collisions even with an explicit ID later in the form');
  assert.equal(resolved.profiles[0].id,'worker-bbbbbbbb');
  assert.equal(resolved.profiles[1].id,'worker-aaaaaaaa');
} finally { crypto.getRandomValues = getRandomValues; }
console.log('ok - omitted profile IDs derive bounded slugs with random suffixes, avoid collisions and preserve saved IDs/bindings');
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
const namedKeys = inventory.keys.map(key => ({...key, name:'Production assistant'}));
assert.deepEqual(profileKeyOptions(namedKeys,inventory.projects,'PRODUCTION assistant research').map(k=>k.id),[id]);
assert.equal(profileKeyOptions(namedKeys,inventory.projects,'production').length,2,'duplicate names retain distinct UUIDs');
assert.equal(profileKeyOptions(namedKeys,inventory.projects,id)[0].name,'Production assistant');
assert.equal(profileKeyOptions(namedKeys,inventory.projects,'rk_employee')[0].id,id,'prefix search remains available after naming');
assert.equal(profileKeyOptions([{...inventory.keys[0],name:null}],inventory.projects,'rk_employee')[0].name,'rk_employee');

assert.deepEqual(profileKeyOptions(inventory.keys,inventory.projects,'','project-two').map(k=>k.id),[secondId]);
assert.equal(profileKeyOptions(inventory.keys,inventory.projects,'robot')[0].status,'Disabled');
assert.deepEqual(selectedKeyIds(' ABC ,\n DEF '),['abc','def']);
assert.doesNotMatch(profileRow(),/<textarea/);
assert.doesNotMatch(profileRow(),/data-key-search|data-key-results/,'search/results belong only in the popup');
assert.match(profileRow(),/aria-haspopup="dialog"/);
const makePicker=value=>{
  const nodes={'[data-profile-keys]':{value},'[data-selected-keys]':{innerHTML:''},'[data-open-keys]':{focus(){this.focused=true;}}};
  return {isConnected:true,nodes,querySelector:selector=>nodes[selector]};
};
const firstPicker=makePicker(id),secondPicker=makePicker('');
const pickerRoot={dataset:{keyProject:''},querySelector:()=>({value:'profiles'})};
const pickerEditor={isConnected:true,closest:()=>pickerRoot,querySelectorAll:selector=>selector==='[data-profile-keys]'?[firstPicker.nodes[selector],secondPicker.nodes[selector]]:[firstPicker,secondPicker]};
await loadProfileKeys(pickerEditor,async()=>inventory);
assert.match(firstPicker.nodes['[data-selected-keys]'].innerHTML,/rk_employee.*Research/s);
console.log('ok - profile key catalog and selected summaries preserve assignments');

// Errors name the offending profile/field and the corrective action.
const validProfile = () => {
  const data = new FormData();
  data.append('profile_slot', 'one');
  for (const [field, value] of Object.entries({id:'staff',name:'Staff',type:'entra_and_relayna_key',audience:'api://staff'})) data.set(`one.${field}`, value);
  return data;
};
for (const [field, value, message] of [
  ['name', '', /Profile 1: enter a Profile name/],
  ['name', 'é'.repeat(61), /Profile name is too long.*Shorten/],
  ['name', 'Staff\u0085Name', /contains control characters.*retype/],
  ['id', 'bad id', /Stable profile ID must use.*Correct it/],
  ['type', 'invalid', /Authentication type/],
  ['audience', '', /Profile 1.*Staff.*enter a Profile audience/],
  ['audience', 'api://with space', /audience must contain no whitespace/],
  ['audience', 'api://with\u0085space', /audience must contain no whitespace/],
  ['audience', 'é'.repeat(257), /at most 512 UTF-8 bytes/],
  ['required_scopes', 'two words', /Scopes value.*Separate values with commas/],
  ['required_roles', 'é'.repeat(257), /Roles value.*512 UTF-8 bytes/],
  ['allowed_groups', Array(65).fill('group').join(','), /Groups allows at most 64 values.*Remove/],
  ['keys', 'invalid', /invalid UUID.*Select keys/],
]) {
  const data = validProfile(); data.set(`one.${field}`, value);
  assert.throws(() => profilesFromForm(data), message);
}
const utf8Boundary = validProfile();
utf8Boundary.set('one.name', 'é'.repeat(60));
utf8Boundary.set('one.audience', 'é'.repeat(256));
assert.equal(profilesFromForm(utf8Boundary).profiles[0].name.length, 60);
const duplicateName = validProfile();
duplicateName.append('profile_slot', 'two');
for(const [field,value] of Object.entries({id:'other',name:'STAFF',type:'relayna_key_only'})) duplicateName.set(`two.${field}`,value);
assert.throws(() => profilesFromForm(duplicateName), /Profile 2.*Profile name is already used.*Choose a different name/);
for (const [raw, expected] of [
  ['authentication_profile_conflict: Authentication configuration changed. Reload before saving.', /draft was not saved.*close this editor without saving and reopen/],
  ['store_unavailable: Store unavailable.', /Keep this draft open and retry Save/],
  ['control_state_unavailable: unavailable', /cannot access saved settings/],
  ['Unexpected gateway response: 502', /Unexpected gateway response: 502/],
]) {
  showProfileError({querySelector:()=>notice},raw);
  assert.match(notice.textContent,expected);
}
