const diagramSources = new WeakMap();
let diagramRender = Promise.resolve();
const renderMermaid = () => {
  if (!window.mermaid) return;
  const nodes = [...document.querySelectorAll(".mermaid")];
  for (const node of nodes) {
    if (!diagramSources.has(node)) diagramSources.set(node, node.textContent);
  }
  // Theme changes and instant navigation may arrive during a render. Restore
  // the original text and serialize work so a finished SVG is never the input.
  diagramRender = diagramRender.catch(() => {}).then(async () => {
    await document.fonts?.ready;
    const connected = nodes.filter(node => node.isConnected);
    if (!connected.length) return;
    const styles = getComputedStyle(document.body);
    const color = name => styles.getPropertyValue(`--rn-color-${name}`).trim();
    window.mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: "base",
      themeVariables: {
        darkMode: document.body.dataset.mdColorScheme === "slate",
        fontFamily: '"Relayna Portico", system-ui, sans-serif',
        fontSize: "14px",
        background: color("canvas"),
        primaryColor: color("surface"),
        primaryTextColor: color("text"),
        primaryBorderColor: color("border-control"),
        secondaryColor: color("surface-subtle"),
        tertiaryColor: color("surface-selected"),
        lineColor: color("text-muted"),
      },
    });
    for (const node of connected) {
      node.textContent = diagramSources.get(node);
      node.removeAttribute("data-processed");
    }
    await window.mermaid.run({ nodes: connected });
  }).catch(error => console.error("Documentation diagram could not render", error));
};

if (window.document$) {
  window.document$.subscribe(renderMermaid);
} else {
  window.addEventListener("load", renderMermaid);
}

new MutationObserver(renderMermaid).observe(document.body, {
  attributes: true,
  attributeFilter: ["data-md-color-scheme"],
});
