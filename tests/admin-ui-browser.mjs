// Run through the supported Chrome browser runtime, with an authenticated tab
// pointed at the disposable gateway environment documented in the QA report.
// This module does not open a browser or access credentials/session storage.
import assert from 'node:assert/strict';
export async function checkAdminSurfaces(tab, selectedNames = null) {
  const results = [];
  const names = ['Overview','Traffic','Usage & cost','Health','Projects','Services','Providers','Routes','Virtual keys','Policies & guardrails','People & identities','Audit log','Settings'];
  for (const name of selectedNames || names) {
    if (await tab.playwright.locator("#sidebar").getAttribute("inert") !== null) await tab.playwright.getByRole("button",{name:"Open navigation",exact:true}).click();
    await tab.playwright.getByRole("navigation",{name:"Portal sections"}).getByRole('button',{name,exact:true}).click();
    await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
    const body = await tab.playwright.locator('#content').innerText();
    const title = await tab.playwright.locator('#view-title').innerText();
    assert.equal(title,name);
    assert.ok(body.trim().length,`${name}: blank content`);
    assert.doesNotMatch(body,/Cannot read properties|is not defined|request_timeout|Internal Server Error/);
    const width = await tab.playwright.evaluate(()=>({page:document.documentElement.scrollWidth,viewport:innerWidth}));
    assert.ok(width.page <= width.viewport,`${name}: page overflows ${width.page}/${width.viewport}`);
    results.push({surface:name,title,bodyLength:body.length,width:width.page});
  }
  if (selectedNames && !selectedNames.includes("People & identities")) return results;
  if (await tab.playwright.locator("#sidebar").getAttribute("inert") !== null) await tab.playwright.getByRole("button",{name:"Open navigation",exact:true}).click();
  await tab.playwright.getByRole('button',{name:'People & identities',exact:true}).click();
  await tab.playwright.getByRole('button',{name:'Workload identities',exact:true}).click();
  await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
  assert.match(await tab.playwright.locator('#content').innerText(),/Service bindings[\s\S]*Project bindings/);
  results.push({surface:'Managed identities',reachable:'People → Workload identities'});
  return results;
}
export async function checkContextAndDrawers(tab) {
  await tab.playwright.getByRole('button',{name:'Projects',exact:true}).click();
  await tab.playwright.getByRole('button',{name:'Create project',exact:true}).click();
  await tab.playwright.getByRole('dialog',{name:'Create project'}).getByLabel('Name',{exact:true}).fill('Disposable UI 3 draft');
  await tab.playwright.getByRole('button',{name:'Close Create project',exact:true}).click();
  await tab.playwright.getByRole('button',{name:'Create project',exact:true}).click();
  assert.equal(await tab.playwright.getByRole('dialog',{name:'Create project'}).getByLabel('Name',{exact:true}).getAttribute('name'),'name');
  const value = await tab.playwright.getByRole('dialog',{name:'Create project'}).getByLabel('Name',{exact:true}).evaluate(node=>node.value);
  assert.equal(value,'Disposable UI 3 draft','closing/reopening must retain input');
  await tab.playwright.getByRole('button',{name:'Close Create project',exact:true}).click();
  await tab.playwright.getByRole('button',{name:'Overview',exact:true}).click();
  await tab.playwright.getByRole('dialog',{name:'Discard unsaved changes?'}).getByRole('button',{name:'Cancel',exact:true}).click();
  assert.equal(await tab.playwright.locator('#view-title').innerText(),'Projects');
  await tab.playwright.getByRole('button',{name:'Overview',exact:true}).click();
  await tab.playwright.getByRole('dialog',{name:'Discard unsaved changes?'}).getByRole('button',{name:'Confirm',exact:true}).click();
  await tab.playwright.getByRole('combobox',{name:'Project scope',exact:true}).selectOption({label:'Analytics Platform'});
  await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
  assert.match(await tab.url(),/project=10000000-0000-0000-0000-000000000001/);
  await tab.playwright.getByRole('button',{name:'Usage & cost',exact:true}).click();
  await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
  assert.match(await tab.url(),/project=10000000-0000-0000-0000-000000000001/);
  await tab.playwright.getByRole('button',{name:'Refresh current view',exact:true}).click();
  await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
  const project = await tab.playwright.getByRole('combobox',{name:'Project scope',exact:true}).evaluate(node=>node.value);
  assert.equal(project,'10000000-0000-0000-0000-000000000001');
  return ['creation draft survives drawer close','dirty navigation cancel preserves page','confirmed navigation discards draft','project context follows Usage','refresh preserves scope'];
}

// configureFaults writes the local QA proxy's path → response/delay map.
// Keep it outside the browser so no page credentials or application state are read.
export async function checkUsageRecovery(tab, configureFaults) {
  await configureFaults({'/admin-ui/admin/usage/events': {status:503}});
  try {
    await tab.playwright.getByRole('button',{name:'Usage & cost',exact:true}).click();
    await tab.playwright.locator('#usage-results[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
    assert.match(await tab.playwright.locator('#usage-results').innerText(),/Usage could not load/);
  } finally { await configureFaults({}); }
  await tab.playwright.locator('#usage-results').getByRole('button',{name:'Retry',exact:true}).click();
  await tab.playwright.locator('#usage-results[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
  assert.match(await tab.playwright.locator('#usage-results').innerText(),/Applied filters:/);
  return 'Usage failure exits loading; Retry restores filtered results';
}

// Requires the disposable admin identity to also own one seeded project.
export async function checkWorkspaceCancellation(tab) {
  await tab.playwright.getByRole('button',{name:'Projects',exact:true}).click();
  await tab.playwright.getByRole('button',{name:'Create project',exact:true}).click();
  await tab.playwright.getByRole('dialog',{name:'Create project'}).getByLabel('Name',{exact:true}).fill('Workspace cancellation draft');
  await tab.playwright.getByRole('button',{name:'Close Create project',exact:true}).click();
  await tab.playwright.locator('#workspace-select').selectOption('owner');
  await tab.playwright.getByRole('dialog',{name:'Discard unsaved changes?'}).getByRole('button',{name:'Cancel',exact:true}).click();
  assert.equal(await tab.playwright.locator('#view-title').innerText(),'Projects');
  assert.equal(await tab.playwright.evaluate(()=>document.querySelector('#workspace-select').value),'admin');
  assert.equal(await tab.playwright.getByRole('navigation',{name:'Portal sections'}).getByRole('button',{name:'Projects',exact:true}).count(),1);
  await tab.playwright.getByRole('button',{name:'Create project',exact:true}).click();
  assert.equal(await tab.playwright.evaluate(()=>document.querySelector('#project-form input[name=name]').value),'Workspace cancellation draft');
  await tab.playwright.getByRole('button',{name:'Close Create project',exact:true}).click();
  await tab.playwright.locator('#workspace-select').selectOption('owner');
  await tab.playwright.getByRole('dialog',{name:'Discard unsaved changes?'}).getByRole('button',{name:'Confirm',exact:true}).click();
  await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached'});
  assert.equal(await tab.playwright.locator('#view-title').innerText(),'My projects');
  assert.equal(await tab.playwright.evaluate(()=>document.querySelector('#workspace-select').value),'owner');
  await tab.playwright.locator('#workspace-select').selectOption('admin');
  await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached'});
  return 'Canceled workspace switch preserves admin shell and draft; confirmed switch enters owner workspace';
}
