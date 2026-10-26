// Run through the supported Chrome browser runtime, with an authenticated tab
// pointed at the disposable gateway environment documented in the QA report.
// This module does not open a browser or access credentials/session storage.
import assert from 'node:assert/strict';

// Run for each loaded surface, in both themes and at desktop/mobile widths.
// Uses rendered geometry: a DOM-only test cannot detect cascade/overlap bugs.
export async function checkComponentAlignment(tab) {
  await tab.playwright.evaluate(() => document.fonts.ready);
  const geometry = await tab.playwright.evaluate(() => {
    const visible = node => node.getClientRects().length && getComputedStyle(node).visibility !== 'hidden';
    const rect = node => {
      const { left, right, top, bottom, width } = node.getBoundingClientRect();
      return { left, right, top, bottom, width };
    };
    return {
      font: getComputedStyle(document.body).fontFamily,
      compactTrends: [...document.querySelectorAll('.monitor-mini-trend')].map(node => ({
        visible: Boolean(visible(node)), text: node.textContent,
        canvas: rect(node.querySelector('canvas')),
        captionSize: parseFloat(getComputedStyle(node.querySelector('.monitor-trend-caption')).fontSize),
      })),
      iconButtons: [...document.querySelectorAll('button.icon-button')].filter(visible).map(node => ({
        label: node.getAttribute('aria-label'), box: rect(node),
        icon: node.querySelector('.ti, svg') ? rect(node.querySelector('.ti, svg')) : null,
      })),
      metrics: [...document.querySelectorAll('.stat, .insight-metric, .monitor-kpi')].filter(visible).map(node => ({
        label: node.textContent.trim(), box: rect(node),
        cardInset: node.matches('.panel.stat') && !node.closest('.monitor-history-summary')
          ? Math.min(...['paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft'].map(side => parseFloat(getComputedStyle(node)[side]))) : null,
        text: [...node.querySelectorAll(':scope > span, :scope > strong, .insight-label, .monitor-kpi-label, .monitor-kpi-value')].filter(visible).map(rect),
      })),
      flatPanels: [...document.querySelectorAll('.monitor-history-summary .stat, .drawer-body > .panel')].filter(visible).map(node => ({
        label: node.textContent.trim(), background: getComputedStyle(node).backgroundColor,
        shadow: getComputedStyle(node).boxShadow, border: getComputedStyle(node).borderTopWidth,
      })),
      footers: [...document.querySelectorAll('.drawer-body form.form-grid > .form-actions')].filter(visible).map(node => ({
        column: getComputedStyle(node).gridColumn, box: rect(node), form: rect(node.parentElement),
        // In scrolling forms, pinned actions intentionally cover offscreen
        // content. Short forms must put actions entirely below their fields.
        fields: node.closest('.drawer-body').scrollHeight <= node.closest('.drawer-body').clientHeight + 1
          ? [...node.parentElement.children].filter(child => child !== node && visible(child)).map(rect) : [],
      })),
    };
  });
  assert.match(geometry.font, /^"?Manrope"?[, ]/, 'interface text must use Manrope');
  for (const trend of geometry.compactTrends) {
    assert.ok(trend.visible && trend.canvas.bottom - trend.canvas.top >= 140, 'compact arrival-rate chart must remain readable and visible on mobile');
    assert.ok(trend.captionSize >= 12, 'compact trend context must use readable text');
    assert.match(trend.text, /Arrival rate/);
    assert.match(trend.text, /req\/s|No fresh rate sample/);
    assert.match(trend.text, /Rolling .+ rate · browser observations/);
  }
  for (const button of geometry.iconButtons) {
    assert.ok(Math.abs(button.box.width - (button.box.bottom - button.box.top)) <= 1, `${button.label}: icon button must be square`);
    assert.ok(button.icon, `${button.label}: icon button needs an icon`);
    assert.ok(Math.abs(button.icon.left + button.icon.right - button.box.left - button.box.right) <= 2, `${button.label}: icon is not horizontally centered`);
    assert.ok(Math.abs(button.icon.top + button.icon.bottom - button.box.top - button.box.bottom) <= 2, `${button.label}: icon is not vertically centered`);
  }
  for (const metric of geometry.metrics) {
    if (metric.cardInset !== null) assert.ok(metric.cardInset >= 12, `${metric.label}: card text needs an inset on every edge`);
    for (const text of metric.text) {
      assert.ok(text.left >= metric.box.left - 1 && text.right <= metric.box.right + 1, `${metric.label}: text escapes metric horizontally`);
      assert.ok(text.top >= metric.box.top - 1 && text.bottom <= metric.box.bottom + 1, `${metric.label}: text escapes metric vertically`);
    }
  }
  for (const panel of geometry.flatPanels) {
    assert.equal(panel.background, 'rgba(0, 0, 0, 0)', `${panel.label}: flat content regains a card background`);
    assert.equal(panel.shadow, 'none', `${panel.label}: flat content regains a card shadow`);
    assert.equal(panel.border, '0px', `${panel.label}: flat content regains a card border`);
  }
  for (const footer of geometry.footers) {
    assert.equal(footer.column, '1 / -1', 'drawer actions must span the form');
    assert.ok(footer.box.left <= footer.form.left + 1 && footer.box.right >= footer.form.right - 1, 'drawer action row must cover the form width');
    for (const field of footer.fields) assert.ok(footer.box.top >= field.bottom - 1, 'drawer actions overlap a field');
  }
  return { iconButtons: geometry.iconButtons.length, metrics: geometry.metrics.length, flatPanels: geometry.flatPanels.length, footers: geometry.footers.length };
}

export async function checkAdminSurfaces(tab, selectedNames = null) {
  const results = [];
  const names = ['Overview','Traffic','Usage & cost','Health','Projects','Services','Providers','Routes','Virtual keys','Policies & guardrails','People & identities','Audit log','Settings'];
  for (const name of selectedNames || names) {
    if (await tab.playwright.locator("#sidebar").getAttribute("inert") !== null) await tab.playwright.getByRole("button",{name:"Open navigation",exact:true}).click();
    await tab.playwright.getByRole("navigation",{name:"Portal sections"}).getByRole('button',{name,exact:true}).click();
    await tab.playwright.locator('#view-title').filter({hasText:name}).waitFor({state:'visible',timeoutMs:15000});
    await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
    const body = await tab.playwright.locator('#content').innerText();
    const title = await tab.playwright.locator('#view-title').innerText();
    assert.equal(title,name);
    assert.ok(body.trim().length,`${name}: blank content`);
    assert.doesNotMatch(body,/Cannot read properties|is not defined|request_timeout|Internal Server Error/);
    const width = await tab.playwright.evaluate(()=>({page:document.documentElement.scrollWidth,viewport:innerWidth}));
    assert.ok(width.page <= width.viewport,`${name}: page overflows ${width.page}/${width.viewport}`);
    const alignment = await checkComponentAlignment(tab);
    results.push({surface:name,title,bodyLength:body.length,width:width.page,alignment});
    if (name === 'Overview') {
      const inventory = tab.playwright.locator('.workflow-disclosure').filter({hasText:'Gateway inventory'});
      const wasOpen = await inventory.getAttribute('open') !== null;
      if (!wasOpen) await inventory.locator('summary').click();
      const expanded = await checkComponentAlignment(tab);
      assert.ok(await inventory.locator('td button.icon-button').count() > 0, 'Overview inventory must exercise table action icons');
      results.push({surface:'Overview inventory',alignment:expanded});
      if (!wasOpen) await inventory.locator('summary').click();
    }
  }
  if (selectedNames && !selectedNames.includes("People & identities")) return results;
  if (await tab.playwright.locator("#sidebar").getAttribute("inert") !== null) await tab.playwright.getByRole("button",{name:"Open navigation",exact:true}).click();
  await tab.playwright.getByRole('button',{name:'People & identities',exact:true}).click();
  await tab.playwright.getByRole('button',{name:'Workload identities',exact:true}).click();
  await tab.playwright.locator('#view-title').filter({hasText:'Managed identities'}).waitFor({state:'visible',timeoutMs:15000});
  await tab.playwright.locator('#content[aria-busy="false"]').waitFor({state:'attached',timeoutMs:15000});
  assert.match(await tab.playwright.locator('#content').innerText(),/Service bindings[\s\S]*Project bindings/);
  results.push({surface:'Managed identities',reachable:'People → Workload identities',alignment:await checkComponentAlignment(tab)});
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
