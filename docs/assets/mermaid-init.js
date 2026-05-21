const renderMermaid = () => {
  if (!window.mermaid) {
    return;
  }
  window.mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
  window.mermaid.run({ querySelector: ".mermaid" });
};

if (window.document$) {
  window.document$.subscribe(renderMermaid);
} else {
  window.addEventListener("load", renderMermaid);
}
