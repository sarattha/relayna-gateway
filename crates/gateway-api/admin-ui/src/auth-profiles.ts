// Profiles are route-local; binding a key never changes its route permissions.
const escape = value => String(value ?? '').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;').replaceAll("'",'&#039;');
let slot = 0;
export function profileRow(profile = {}, bindings = []) {
  const id = `profile-${++slot}`;
  const entra = profile.entra || {};
  const keys = bindings.filter(binding => binding.profile_id === profile.id).map(binding => binding.key_id);
  return `<fieldset class="authentication-profile form-grid" data-profile-row>
    <legend>Authentication profile</legend><input type="hidden" name="profile_slot" value="${id}">
    <label>Stable profile ID<input name="${id}.id" value="${escape(profile.id)}" maxlength="64" pattern="(?:[A-Za-z0-9_]|-)+"></label>
    <label>Profile name<input name="${id}.name" value="${escape(profile.name)}" maxlength="120"></label>
    <label>Authentication type<select name="${id}.type"><option value="entra_and_relayna_key">Entra + Relayna key</option><option value="relayna_key_only" ${profile.type === 'relayna_key_only' ? 'selected' : ''}>Relayna key only</option></select></label>
    <label class="check"><input type="checkbox" name="${id}.enabled" ${profile.enabled !== false ? 'checked' : ''}> Enabled</label>
    <label>Profile audience<input name="${id}.audience" value="${escape(entra.audience)}"></label>
    ${['required_scopes','required_roles','allowed_groups'].map(name => `<label>${escape(name.replaceAll('_',' '))}<input name="${id}.${name}" value="${escape((entra[name] || []).join(', '))}"></label>`).join('')}
    <label class="check"><input type="checkbox" name="${id}.allow_apigee" ${entra.allow_apigee ? 'checked' : ''}> Accept signed Apigee identity</label>
    <label class="wide-field">Assigned key UUIDs<textarea name="${id}.keys" rows="2">${escape(keys.join('\n'))}</textarea></label>
    <p class="help wide-field">${keys.length} saved bindings. One UUID per line or comma; a key may appear in only one profile on this route. Disabling blocks every assigned key. Remove assignments explicitly before removing a profile. Key only still requires an authenticated Relayna key. Audience and claim fields apply only to Entra profiles.</p>
    <button type="button" data-remove-profile>Remove unbound profile</button>
  </fieldset>`;
}
export function profileFields(access = {}) {
  const set = access.authentication_profiles;
  return `<section class="wide-field" data-profile-editor><div class="panel-heading"><h4>Authentication profiles</h4><div class="actions"><button type="button" data-add-profile>Add profile</button></div></div>
    <p class="help">Choose Explicit authentication profiles above to opt in. Unassigned keys are rejected. Profiles govern identity independently of forwarding mode; canonical aliases share these assignments. Existing gateway setting and single-policy modes keep legacy behavior.</p>
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
export function bindProfileEditor(doc = document) {
  doc.addEventListener('click', event => {
    const editor = event.target.closest('[data-profile-editor]');
    if (!editor) return;
    if (event.target.closest('[data-add-profile]')) {
      const rows = editor.querySelector('[data-profile-rows]');
      rows.insertAdjacentHTML('beforeend',profileRow());
      rows.lastElementChild.querySelector('input:not([type=hidden])').focus();
    }
    if (event.target.closest('[data-remove-profile]')) {
      const row = event.target.closest('[data-profile-row]');
      if (row.querySelector('textarea').value.trim()) { editor.querySelector('[data-profile-notice]').textContent = 'Remove this profile’s key assignments explicitly before removing it.'; return; }
      row.remove();
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
