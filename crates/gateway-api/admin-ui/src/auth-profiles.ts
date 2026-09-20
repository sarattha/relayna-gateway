// Profiles are route-local; binding a key never changes its route permissions.
const escape = value => String(value ?? '').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;').replaceAll("'",'&#039;');
let slot = 0;
export function profileRow(profile = {}, bindings = []) {
  const id = `profile-${++slot}`;
  const entra = profile.entra || {};
  const keyOnly = profile.type === "relayna_key_only";
  const keys = bindings.filter(binding => binding.profile_id === profile.id).map(binding => binding.key_id);
  return `<fieldset class="authentication-profile form-grid" data-profile-row>
    <legend>Authentication profile</legend><input type="hidden" name="profile_slot" value="${id}">
    <input type="hidden" name="${id}.original_id" value="${escape(profile.id)}">
    <label>Stable profile ID (optional)<input placeholder="Generated from profile name on save" name="${id}.id" value="${escape(profile.id)}" maxlength="64" pattern="(?:[A-Za-z0-9_]|-)+"></label>
    <label>Profile name (required)<input required placeholder="Internal automation" name="${id}.name" value="${escape(profile.name)}" maxlength="120"></label>
    <label>Profile authentication<select data-profile-type name="${id}.type"><option value="entra_and_relayna_key">Entra + Relayna key</option><option value="relayna_key_only" ${profile.type === 'relayna_key_only' ? 'selected' : ''}>Relayna key only</option></select></label>
    <label class="check"><input type="checkbox" name="${id}.enabled" ${profile.enabled !== false ? 'checked' : ''}> Enabled</label>
    <div class="wide-field form-grid" data-profile-entra ${keyOnly ? 'hidden' : ''}>
    <label>Profile audience (required)<input ${keyOnly ? 'disabled' : 'required'} name="${id}.audience" value="${escape(entra.audience)}" placeholder="api://employees"></label>
    ${['required_scopes','required_roles','allowed_groups'].map(name => `<label>${escape({required_scopes:'Scopes (optional)',required_roles:'Roles (optional)',allowed_groups:'Groups (optional)'}[name])}<input ${keyOnly ? 'disabled' : ''} name="${id}.${name}" value="${escape((entra[name] || []).join(', '))}"></label>`).join('')}
    <label class="check"><input type="checkbox" ${keyOnly ? 'disabled' : ''} name="${id}.allow_apigee" ${entra.allow_apigee ? 'checked' : ''}> Accept signed Apigee identity</label></div>
    <p class="help wide-field" data-profile-key-only ${keyOnly ? '' : 'hidden'}>Authenticates the assigned Relayna key. No Entra JWT, audience or claims are used.</p>
    <section class="wide-field profile-key-picker" data-key-picker>
      <input type="hidden" name="${id}.keys" data-profile-keys value="${escape(keys.join(','))}">
      <div class="panel-heading"><span class="profile-key-label">Assigned keys</span><button type="button" data-open-keys aria-haspopup="dialog">Select keys</button></div>
      <div data-selected-keys>${keys.map(key => `<div class="profile-key-item"><code>${escape(key)}</code></div>`).join('') || '<p class="help">No keys assigned.</p>'}</div>
    </section>
    <button type="button" data-remove-profile>Remove unbound profile</button>
  </fieldset>`;
}
export function profileFields(access = {}) {
  const set = access.authentication_profiles;
  return `<section class="wide-field" data-profile-editor ${set ? '' : 'hidden'}><div class="panel-heading"><h4>Authentication profiles</h4><div class="actions"><button type="button" data-add-profile>Add profile</button></div></div>
    <p class="help">A profile can have multiple keys. Each key can belong to only one profile on this route. Unassigned keys are denied.</p>
    <input type="hidden" name="profiles_revision" value="${set?.revision || 0}">
    <div data-profile-rows>${(set?.profiles || []).map(profile => profileRow(profile,set.bindings)).join('')}</div>
    <p role="status" data-profile-notice></p></section>`;
}
export function generateProfileId(name, reserved) {
  const slug = name.normalize('NFKD').replace(/[\u0300-\u036f]/g, '').toLowerCase()
    .replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 55).replace(/-+$/g, '') || 'profile';
  let id;
  do {
    const suffix = crypto.getRandomValues(new Uint32Array(1))[0].toString(16).padStart(8, '0');
    id = `${slug}-${suffix}`;
  } while (reserved.has(id));
  return id;
}
export function profilesFromForm(form, accessa = false) {
  const profiles = [], bindings = [], ids = new Set(), names = new Set(), keys = new Set();
  const split = value => String(value || '').split(/[,\n]/).map(value => value.trim()).filter(Boolean);
  const bytes = value => new TextEncoder().encode(value).length;
  const reserved = new Set(form.getAll('profile_slot').map(slot => String(form.get(`${slot}.id`) || '').trim() || String(form.get(`${slot}.original_id`) || '').trim()));
  for (const slot of form.getAll('profile_slot')) {
    const read = name => String(form.get(`${slot}.${name}`) || '').trim();
    const name = read('name'), type = read('type');
    const id = read('id') || read('original_id') || generateProfileId(name, reserved);
    reserved.add(id);
    const label = `Profile ${profiles.length + 1}${name ? ` (“${name}”)` : ''}`;
    if (!name) throw new Error(`${label}: enter a Profile name.`);
    if (bytes(name) > 120) throw new Error(`${label}: Profile name is too long. Shorten it to 120 UTF-8 bytes or fewer; some characters use more than one byte.`);
    if (/\p{Cc}/u.test(name)) throw new Error(`${label}: Profile name contains control characters. Remove them or retype the name.`);
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(id)) throw new Error(`${label}: Stable profile ID must use 1–64 letters, numbers, hyphens or underscores. Correct it, or leave it blank to generate one.`);
    if (ids.has(id)) throw new Error(`${label}: Stable profile ID “${id}” is already used by another profile on this route. Choose a different ID.`);
    if (names.has(name.toLowerCase())) throw new Error(`${label}: Profile name is already used on this route (ignoring case). Choose a different name.`);
    if (!['entra_and_relayna_key','relayna_key_only'].includes(type)) throw new Error(`${label}: select Entra + Relayna key or Relayna key only under Profile authentication.`);
    if (accessa && type !== 'entra_and_relayna_key') throw new Error(`${label}: Accessa profiles require Entra + Relayna key. Change Profile authentication and enter a Profile audience.`);
    ids.add(id); names.add(name.toLowerCase());
    const profile = {id,name,type,enabled:form.has(`${slot}.enabled`)};
    if (type === 'entra_and_relayna_key') {
      const audience = read('audience');
      if (!audience) throw new Error(`${label}: enter a Profile audience for Entra + Relayna key, such as api://employees.`);
      if (/\p{White_Space}/u.test(audience) || bytes(audience) > 512) throw new Error(`${label}: Profile audience must contain no whitespace and be at most 512 UTF-8 bytes. Correct the audience.`);
      profile.entra = {audience,allow_apigee:form.has(`${slot}.allow_apigee`)};
      for (const field of ['required_scopes','required_roles','allowed_groups']) {
        const claims = split(read(field));
        const fieldLabel = {required_scopes:'Scopes',required_roles:'Roles',allowed_groups:'Groups'}[field];
        if (claims.length > 64) throw new Error(`${label}: ${fieldLabel} allows at most 64 values. Remove extra entries.`);
        if (claims.some(claim => bytes(claim) > 512 || /\p{White_Space}/u.test(claim))) throw new Error(`${label}: each ${fieldLabel} value must contain no whitespace and be at most 512 UTF-8 bytes. Separate values with commas.`);
        profile.entra[field] = claims;
      }
    }
    profiles.push(profile);
    for (const key of split(read('keys'))) {
      const key_id = key.toLowerCase();
      if (!/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(key_id)) throw new Error(`${label}: an assigned key has an invalid UUID. Remove that assignment and select the key again using Select keys.`);
      if (keys.has(key_id)) throw new Error(`${label}: key ${key_id} is assigned more than once on this route. Remove its other assignment before adding it here.`);
      keys.add(key_id); bindings.push({key_id,profile_id:id});
    }
  }
  if (!profiles.length) throw new Error('Add at least one authentication profile before saving.');
  if (profiles.length > 32) throw new Error('This route has more than 32 authentication profiles. Remove an unused profile before saving.');
  if (bindings.length > 1024) throw new Error('This route has more than 1,024 assigned keys across its profiles. Remove extra assignments before saving.');
  return {revision:Number(form.get('profiles_revision') || 0),profiles,bindings};
}
export function syncIdentityFields(root) {
  const mode = root.querySelector('[name="endpoint_entra_mode"]').value;
  const toggle = (group, active) => {
    group.hidden = !active;
    group.querySelectorAll('input, select, textarea, button').forEach(control => { control.disabled = !active; });
  };
  const endpoint = root.querySelector('[data-endpoint-entra]');
  toggle(endpoint, mode === 'required');
  endpoint.querySelector('[name="endpoint_audience"]').required = mode === 'required';
  const editor = root.querySelector('[data-profile-editor]');
  toggle(editor, mode === 'profiles');
  for (const row of editor.querySelectorAll('[data-profile-row]')) {
    const entra = row.querySelector('[data-profile-type]').value === 'entra_and_relayna_key';
    toggle(row.querySelector('[data-profile-entra]'), mode === 'profiles' && entra);
    row.querySelector('[name$=".audience"]').required = mode === 'profiles' && entra;
    row.querySelector('[data-profile-key-only]').hidden = entra;
  }
}

const keyCatalogs = new WeakMap();
export const selectedKeyIds = value => String(value || '').split(/[,\n]/).map(id => id.trim().toLowerCase()).filter(Boolean);
export function profileKeyOptions(keys, projects, query = '', projectId = '') {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return keys.filter(key => !projectId || key.project_id === projectId).map(key => {
    const project = projects.find(project => project.id === key.project_id);
    const name = key.name || key.key_prefix || key.id;
    const owner = project?.name || key.project_id || 'Individual';
    const status = key.revoked_at ? 'Revoked' : key.disabled ? 'Disabled' : key.expires_at && Date.parse(key.expires_at) <= Date.now() ? 'Expired' : 'Active';
    const searchable = [name,key.key_prefix,key.id,owner,key.project_id,...(key.service_names || [])].join(' ').toLowerCase();
    return {id:key.id,name,owner,status,searchable};
  }).filter(key => terms.every(term => key.searchable.includes(term)));
}
function keyDescription(key) {
  return `<span><span class="profile-key-name">${escape(key.name)}</span><small>${escape(key.owner)} · ${escape(key.status)}</small><code>${escape(key.id)}</code></span>`;
}
export function refreshProfileKeys(editor) {
  const state = keyCatalogs.get(editor);
  if (!state) return;
  const catalog = state.catalog || {keys:[],projects:[]};
  const all = profileKeyOptions(catalog.keys, catalog.projects);
  const root = editor.closest('[data-endpoint-identity]');
  const active = root.querySelector('[name="endpoint_entra_mode"]').value === 'profiles';
  for (const picker of editor.querySelectorAll('[data-key-picker]')) {
    const ids = selectedKeyIds(picker.querySelector('[data-profile-keys]').value);
    picker.querySelector('[data-selected-keys]').innerHTML = ids.map(id => {
      const key = all.find(key => key.id === id) || {id,name:'Key details unavailable',owner:'Saved assignment',status:'Unknown'};
      return `<div class="profile-key-item">${keyDescription(key)}<button type="button" data-remove-key="${escape(id)}" aria-label="Remove ${escape(key.name)} (${escape(id)})" ${active ? '' : 'disabled'}>Remove</button></div>`;
    }).join('') || '<p class="help">No keys assigned.</p>';
  }
}

export async function loadProfileKeys(editor, loadCatalog) {
  if (keyCatalogs.get(editor)?.loading) return;
  const state = {catalog:keyCatalogs.get(editor)?.catalog,loading:true,error:false};
  keyCatalogs.set(editor,state);
  refreshProfileKeys(editor);
  try { state.catalog = await loadCatalog(); }
  catch { state.error = true; }
  finally { state.loading = false; if (editor.isConnected) refreshProfileKeys(editor); }
}

export function bindProfileEditor(doc = document, loadCatalog = async () => ({keys:[],projects:[]}), openKeyPicker) {
  doc.addEventListener('change', event => {
    if (!event.target.matches('[name="endpoint_entra_mode"], [data-profile-type]')) return;
    const root = event.target.closest('[data-endpoint-identity]');
    syncIdentityFields(root);
    refreshProfileKeys(root.querySelector('[data-profile-editor]'));
  });
  doc.addEventListener('click', async event => {
    const editor = event.target.closest('[data-profile-editor]');
    if (!editor) return;
    const picker = event.target.closest('[data-key-picker]');
    if (event.target.closest('[data-open-keys]')) {
      openKeyPicker(picker, editor);
      return;
    }
    const remove = event.target.closest('[data-remove-key]');
    if (remove) {
      const input = picker.querySelector('[data-profile-keys]');
      input.value = selectedKeyIds(input.value).filter(id => id !== remove.dataset.removeKey).join(',');
      refreshProfileKeys(editor);
      picker.querySelector('[data-open-keys]').focus();
      return;
    }
    if (event.target.closest('[data-add-profile]')) {
      const rows = editor.querySelector('[data-profile-rows]');
      rows.insertAdjacentHTML('beforeend',profileRow());
      syncIdentityFields(editor.closest('[data-endpoint-identity]'));
      refreshProfileKeys(editor);
      rows.lastElementChild.querySelector('input:not([type=hidden])').focus();
    }
    if (event.target.closest('[data-remove-profile]')) {
      const row = event.target.closest('[data-profile-row]');
      if (row.querySelector('[data-profile-keys]').value.trim()) { editor.querySelector('[data-profile-notice]').textContent = 'Remove this profile’s key assignments explicitly before removing it.'; return; }
      row.remove();
      refreshProfileKeys(editor);
    }
  });
}

// Keep validation and conflict feedback inside the active dialog's focus boundary.
export function profileErrorMessage(message) {
  if (message.startsWith('authentication_profile_conflict:')) return 'These authentication settings changed after you opened the editor. Your draft was not saved. Copy any changes you want to keep, then close this editor without saving and reopen it to review the latest settings before applying them again.';
  if (message.startsWith('store_unavailable:') || message.startsWith('control_state_unavailable:')) return 'The gateway cannot access saved settings right now. Keep this draft open and retry Save when the service is available. If the result is uncertain, reopen the editor to check the saved settings before retrying.';
  return message;
}
export function showProfileError(root, message) {
  message = profileErrorMessage(message);
  const notice = root.querySelector?.('[data-profile-notice]')
    || root.closest?.('[role=dialog]')?.querySelector('[data-profile-notice], [data-binding-result]');
  if (!notice) return false;
  if (notice.hasAttribute?.('data-react-error')) notice.dispatchEvent(new CustomEvent('profile-error',{bubbles:true,detail:message}));
  else notice.textContent = message;
  notice.setAttribute('role', 'alert');
  notice.setAttribute('tabindex', '-1');
  notice.focus();
  notice.scrollIntoView({block: 'nearest'});
  return true;
}
