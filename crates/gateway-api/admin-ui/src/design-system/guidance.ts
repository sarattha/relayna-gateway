import { fieldGuidance, termGuidance } from "./guidance-content";

// Constrain the tooltip to the viewport, including narrow screens and bottom rows.
export function tooltipPosition(anchor, size, viewport) {
  const margin = 8;
  const left = Math.max(margin, Math.min(anchor.left, viewport.width - size.width - margin));
  const below = anchor.bottom + margin;
  const top = below + size.height <= viewport.height - margin
    ? below : Math.max(margin, anchor.top - size.height - margin);
  return { left, top };
}

function contextFor(control: HTMLElement): string {
  if (control.closest('[data-pricing-rule-row]')) return 'pricing';
  if (control.closest('#traffic-filters')) return 'traffic';
  if (control.closest('#usage-form')) return 'usage';
  if (control.closest('#key-form, #key-edit-form, #policy-layer-form')) return 'policy';
  if (control.closest('#provider-health-state-form')) return 'health';
  if (control.closest('#studio-connection-form')) return 'studio';
  if (control.closest('#service-form, #service-edit-form')) return 'service';
  if (control.closest('#litellm-passthrough-form')) return 'passthrough';
  if (control.closest('.owner-request-filters')) return 'owner';
  return 'configuration';
}

const controls = 'input:not([type="hidden"]), select, textarea';
const terms = 'th, .stat > span:first-child, .posture-fact > span:first-child, .investigation-metrics > div > span:first-child, .investigation-facts dt';

/** Decorate explicit, reviewed fields after dynamic rendering, without replacing
 * controls, changing values, or rebinding their submit/change handlers. */
export function installComponentGuidance(doc: Document = document) {
  const seen = new WeakSet<Element>();
  let sequence = 0;
  let active: { trigger: HTMLElement; tip: HTMLElement; pinned: boolean } | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  const win = doc.defaultView!;
  const clearTimer = () => { if (hideTimer) clearTimeout(hideTimer); hideTimer = undefined; };
  const dismiss = () => {
    clearTimer();
    if (!active) return;
    const { trigger, tip } = active;
    if (tip.matches(':popover-open')) tip.hidePopover();
    tip.remove();
    const ids = (trigger.getAttribute('aria-describedby') || '').split(/\s+/).filter(id => id && id !== tip.id);
    if (ids.length) trigger.setAttribute('aria-describedby', ids.join(' '));
    else trigger.removeAttribute('aria-describedby');
    active = null;
  };
  const scheduleHide = () => {
    clearTimer();
    if (active?.pinned || active?.trigger === doc.activeElement) return;
    hideTimer = setTimeout(dismiss, 180);
  };
  const show = (trigger: HTMLElement) => {
    clearTimer();
    if (active?.trigger === trigger) return;
    dismiss();
    const tip = doc.createElement('span');
    tip.id = `component-tooltip-${++sequence}`;
    tip.className = 'component-tooltip';
    tip.setAttribute('role', 'tooltip');
    tip.setAttribute('popover', 'manual');
    tip.textContent = trigger.dataset.helpTooltip || '';
    // Keep the tooltip inside the same modal/inert subtree as its trigger. The
    // popover top layer escapes table/drawer clipping without escaping semantics.
    trigger.after(tip);
    trigger.setAttribute('aria-describedby', [trigger.getAttribute('aria-describedby'), tip.id].filter(Boolean).join(' '));
    active = { trigger, tip, pinned: false };
    tip.showPopover();
    const rect = trigger.getBoundingClientRect();
    const size = tip.getBoundingClientRect();
    const position = tooltipPosition(rect, size, { width: win.innerWidth, height: win.innerHeight });
    tip.style.left = `${position.left}px`;
    tip.style.top = `${position.top}px`;
    tip.addEventListener('pointerenter', clearTimer);
    tip.addEventListener('pointerleave', scheduleHide);
  };
  const bindTooltip = (trigger: HTMLElement) => {
    trigger.addEventListener('pointerenter', event => { if (event.pointerType !== 'touch') show(trigger); });
    trigger.addEventListener('pointerleave', scheduleHide);
    trigger.addEventListener('focus', () => show(trigger));
    trigger.addEventListener('blur', () => { if (active?.trigger === trigger) { active.pinned = false; scheduleHide(); } });
    if (trigger.classList.contains('help-trigger')) trigger.addEventListener('click', () => {
      if (active?.trigger === trigger && active.pinned) dismiss();
      else { show(trigger); if (active) active.pinned = true; }
    });
  };
  const addTerm = (element: HTMLElement) => {
    if (seen.has(element)) return;
    seen.add(element);
    const label = element.textContent?.trim() || '';
    const explanation = termGuidance[label];
    if (!explanation) return;
    const trigger = doc.createElement('button');
    trigger.type = 'button';
    trigger.className = 'help-trigger';
    trigger.setAttribute('aria-label', `About ${label}`);
    trigger.dataset.helpTooltip = explanation;
    trigger.textContent = '?';
    element.append(' ', trigger);
    bindTooltip(trigger);
  };
  const addField = (control: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement) => {
    if (seen.has(control)) return;
    seen.add(control);
    // Export fields already have individually authored help. Grouped selections
    // have shared guidance, rather than repeating it for every checkbox row.
    if (control.hasAttribute('aria-describedby') || (control.type === 'checkbox' && !control.closest('label.check'))) return;
    const name = control.name || control.dataset.pricingRuleField || control.dataset.endpointField || control.id;
    const explanation = fieldGuidance(name, contextFor(control));
    const label = control.closest('label');
    if (!explanation || !label) return;
    const labelCopy = label.cloneNode(true) as HTMLElement;
    labelCopy.querySelectorAll('input, select, textarea, .field-hint, .subtle').forEach(node => node.remove());
    const title = labelCopy.textContent?.replace(/\s+/g, ' ').trim();
    // Help remains visible within the existing label, including when conditional
    // controls hide it. Keep the accessible name concise and describe separately.
    if (title && !control.hasAttribute('aria-label') && !control.hasAttribute('aria-labelledby')) control.setAttribute('aria-label', title);
    const help = doc.createElement('small');
    help.id = `component-field-help-${++sequence}`;
    help.className = 'field-hint';
    help.textContent = explanation;
    control.setAttribute('aria-describedby', help.id);
    label.append(help);
  };
  const enhance = (root: Element) => {
    if (root.closest('.component-tooltip, .field-hint')) return;
    if (root.matches(controls)) addField(root as HTMLInputElement);
    root.querySelectorAll(controls).forEach(node => addField(node as HTMLInputElement));
    if (root.matches(terms)) addTerm(root as HTMLElement);
    root.querySelectorAll(terms).forEach(node => addTerm(node as HTMLElement));
    const icons = 'button.icon-button[aria-label], #command-trigger';
    const buttons = [...root.querySelectorAll<HTMLElement>(icons)];
    if (root.matches(icons)) buttons.push(root as HTMLElement);
    for (const button of buttons) {
      if (seen.has(button)) continue;
      seen.add(button);
      button.dataset.helpTooltip = button.id === 'command-trigger' ? 'Search available pages. Keyboard shortcut: Command/Ctrl + K.' : button.getAttribute('aria-label') || '';
      bindTooltip(button);
    }
  };
  enhance(doc.body);
  const observer = new MutationObserver(records => {
    if (active && !active.trigger.isConnected) dismiss();
    const roots = new Set<Element>();
    for (const record of records) for (const node of record.addedNodes) if (node instanceof Element) roots.add(node);
    for (const root of roots) if (root.isConnected && ![...roots].some(other => other !== root && other.contains(root))) enhance(root);
  });
  observer.observe(doc.body, { childList: true, subtree: true });
  const escape = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && active) {
      event.preventDefault();
      event.stopImmediatePropagation();
      dismiss();
    }
  };
  const outside = (event: PointerEvent) => {
    if (active && !active.trigger.contains(event.target as Node) && !active.tip.contains(event.target as Node)) dismiss();
  };
  // Capture before a dialog's Escape handler; the first Escape dismisses help.
  win.addEventListener('keydown', escape, true);
  doc.addEventListener('pointerdown', outside, true);
  doc.addEventListener('scroll', dismiss, true);
  win.addEventListener('resize', dismiss);
  return () => {
    observer.disconnect(); dismiss();
    win.removeEventListener('keydown', escape, true);
    doc.removeEventListener('pointerdown', outside, true);
    doc.removeEventListener('scroll', dismiss, true);
    win.removeEventListener('resize', dismiss);
  };
}
