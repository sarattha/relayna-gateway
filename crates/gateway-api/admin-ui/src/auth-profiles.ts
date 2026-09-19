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
    <label>Stable profile ID (required)<input required placeholder="internal-automation" name="${id}.id" value="${escape(profile.id)}" maxlength="64" pattern="(?:[A-Za-z0-9_]|-)+"></label>
    <label>Profile name (required)<input required placeholder="Internal automation" name="${id}.name" value="${escape(profile.name)}" maxlength="120"></label>
    <label>Authentication type<select data-profile-type name="${id}.type"><option value="entra_and_relayna_key">Entra + Relayna key</option><option value="relayna_key_only" ${profile.type === 'relayna_key_only' ? 'selected' : ''}>Relayna key only</option></select></label>
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
export function profilesFromForm(form, accessa = false) {
  const profiles = [], bindings = [], ids = new Set(), names = new Set(), keys = new Set();
  const split = value => String(value || '').split(/[,\n]/).map(value => value.trim()).filter(Boolean);
  for (const slot of form.getAll('profile_slot')) {
    const read = name => String(form.get(`${slot}.${name}`) || '').trim();
    const id = read('id'), name = read('name'), type = read('type');
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(id) || !name || name.length > 120 || /[\x00-\x1f\x7f]/.test(name) || ids.has(id) || names.has(name.toLowerCase())) throw new Error('Use unique profile IDs and names, without control characters.');
    if (!['entra_and_relayna_key','relayna_key_only'].includes(type) || (accessa && type !== 'entra_and_relayna_key')) throw new Error('Accessa profiles require Entra + Relayna key.');
    ids.add(id); names.add(name.toLowerCase());
    const profile = {id,name,type,enabled:form.has(`${slot}.enabled`)};
    if (type === 'entra_and_relayna_key') {
      const audience = read('audience');
      if (!audience || /\s/.test(audience) || audience.length > 512) throw new Error('Each Entra profile requires a valid audience.');
      profile.entra = {audience,allow_apigee:form.has(`${slot}.allow_apigee`)};
      for (const field of ['required_scopes','required_roles','allowed_groups']) {
        const claims = split(read(field));
        if (claims.length > 64 || claims.some(claim => claim.length > 512 || /\s/.test(claim))) throw new Error('Profile claim lists allow at most 64 values without whitespace.');
        profile.entra[field] = claims;
      }
    }
    profiles.push(profile);
    for (const key of split(read('keys'))) {
      const key_id = key.toLowerCase();
      if (!/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(key_id) || keys.has(key_id)) throw new Error('Each assigned key must be a valid UUID and appear only once on this route.');
      keys.add(key_id); bindings.push({key_id,profile_id:id});
    }
  }
  if (!profiles.length || profiles.length > 32 || bindings.length > 1024) throw new Error('Configure 1–32 profiles and at most 1,024 bindings per route.');
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
const keyDialogs = new WeakMap();
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
  keyDialogs.get(editor)?.();
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

export function openProfileKeyDialog(picker, editor, {doc = document, loadCatalog, mountDialog}) {
  const input = picker.querySelector('[data-profile-keys]');
  const original = new Set(selectedKeyIds(input.value));
  const selected = new Set(original);
  const root = editor.closest('[data-endpoint-identity]');
  const project = root.dataset.keyProject || '';
  const elsewhere = () => new Set([...editor.querySelectorAll('[data-profile-keys]')].filter(control => control !== input).flatMap(control => selectedKeyIds(control.value)));
  const backdrop = doc.createElement('section');
  backdrop.className = 'modal-backdrop';
  const title = `profile-key-dialog-${++slot}`;
  backdrop.innerHTML = `<div class="modal profile-key-dialog" role="dialog" aria-modal="true" aria-labelledby="${title}">
    <h3 id="${title}">Select keys</h3>
    <div class="modal-form"><div class="modal-scroll">
      <label>Search keys<input type="search" data-key-search data-guidance-name="profile_keys_search" placeholder="Key prefix, UUID, project or service" autocomplete="off"></label>
      <p class="help" role="status" data-key-results-status></p>
      <button type="button" data-key-retry hidden>Retry loading keys</button>
      <div class="profile-key-results" data-key-results></div>
      <h4 class="profile-key-label" data-key-selection-count>Selected keys</h4><div data-key-draft></div>
      <p class="help">Apply updates this profile’s draft. Save identity to save the assignments.</p>
    </div><div class="form-actions"><button type="button" class="primary" data-key-apply>Apply selection</button><button type="button" data-key-cancel>Cancel</button></div></div>
  </div>`;
  const search = backdrop.querySelector('[data-key-search]');
  const render = () => {
    const state = keyCatalogs.get(editor) || {};
    const catalog = state.catalog || {keys:[],projects:[]};
    const all = profileKeyOptions(catalog.keys,catalog.projects);
    const unavailable = elsewhere();
    const matches = profileKeyOptions(catalog.keys,catalog.projects,search.value,project).filter(key => !selected.has(key.id) && !unavailable.has(key.id));
    backdrop.querySelector('[data-key-results-status]').textContent = state.loading ? 'Loading keys…' : state.error ? 'Could not load keys. Your selections are kept.' : `${matches.length} available matching keys${matches.length > 30 ? ' · showing the first 30; refine your search' : ''}. Keys already assigned to a profile are excluded.`;
    backdrop.querySelector('[data-key-retry]').hidden = !state.error;
    backdrop.querySelector('[data-key-apply]').disabled = !state.catalog || state.loading || state.error;
    backdrop.querySelector('[data-key-results]').innerHTML = state.loading || state.error ? '' : matches.slice(0,30).map(key => `<div class="profile-key-item">${keyDescription(key)}<button type="button" data-dialog-add-key="${escape(key.id)}" aria-label="Add ${escape(key.name)} (${escape(key.id)})">Add</button></div>`).join('');
    backdrop.querySelector('[data-key-selection-count]').textContent = `Selected keys (${selected.size})`;
    backdrop.querySelector('[data-key-draft]').innerHTML = [...selected].map(id => {
      const key = all.find(key => key.id === id) || {id,name:'Key details unavailable',owner:'Saved assignment',status:'Unknown'};
      return `<div class="profile-key-item">${keyDescription(key)}<button type="button" data-dialog-remove-key="${escape(id)}" aria-label="Remove ${escape(key.name)} (${escape(id)})">Remove</button></div>`;
    }).join('') || '<p class="help">No keys selected.</p>';
  };
  doc.body.appendChild(backdrop);
  keyDialogs.set(editor,render);
  const close = mountDialog(backdrop, {initialFocus:'[data-key-search]',restoreFocus:picker.querySelector('[data-open-keys]'),onClose:() => {if (keyDialogs.get(editor) === render) keyDialogs.delete(editor);}});
  search.addEventListener('input',render);
  search.addEventListener('keydown',event => {if (event.key === 'Enter') event.preventDefault();});
  backdrop.querySelector('[data-key-cancel]').addEventListener('click',() => close());
  backdrop.querySelector('[data-key-retry]').addEventListener('click',() => {void loadProfileKeys(editor,loadCatalog);});
  backdrop.querySelector('[data-key-apply]').addEventListener('click',() => {
    const state = keyCatalogs.get(editor);
    if (!state?.catalog || state.loading || state.error || !picker.isConnected) return;
    const allowed = new Set(profileKeyOptions(state.catalog.keys,state.catalog.projects,'',project).map(key => key.id));
    const unavailable = elsewhere();
    if ([...selected].some(id => unavailable.has(id) || (!original.has(id) && !allowed.has(id)))) {render();return;}
    input.value = [...selected].join(',');
    refreshProfileKeys(editor);
    close();
  });
  backdrop.addEventListener('click',event => {
    const add = event.target.closest('[data-dialog-add-key]');
    const remove = event.target.closest('[data-dialog-remove-key]');
    if (add) {
      const state = keyCatalogs.get(editor);
      const id = add.dataset.dialogAddKey;
      if (!state?.catalog || state.loading || state.error || elsewhere().has(id) || !profileKeyOptions(state.catalog.keys,state.catalog.projects,'',project).some(key => key.id === id)) return;
      selected.add(id);
      search.value = '';
    } else if (remove) selected.delete(remove.dataset.dialogRemoveKey);
    else return;
    render();
    search.focus();
  });
  render();
  void loadProfileKeys(editor,loadCatalog);
  return backdrop;
}

export function bindProfileEditor(doc = document, loadCatalog = async () => ({keys:[],projects:[]}), mountDialog) {
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
      openProfileKeyDialog(picker, editor, {doc, loadCatalog, mountDialog});
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
export function showProfileError(root, message) {
  const notice = root.querySelector?.('[data-profile-notice]')
    || root.closest?.('[role=dialog]')?.querySelector('[data-profile-notice], [data-binding-result]');
  if (!notice) return false;
  notice.textContent = message;
  notice.setAttribute('role', 'alert');
  notice.setAttribute('tabindex', '-1');
  notice.focus();
  notice.scrollIntoView({block: 'nearest'});
  return true;
}
