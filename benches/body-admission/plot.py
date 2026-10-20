#!/usr/bin/env python3
"""Export a shareable benchmark comparison: uv run --with matplotlib ..."""

import argparse
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

plt.rcParams.update({"svg.fonttype": "none", "font.size": 10})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    summaries = json.loads(Path(args.summary).read_text())
    if len(summaries) != 2:
        parser.error("expected baseline and improved summaries")
    colors = ["#2563eb", "#0d9488"]
    fig = plt.figure(figsize=(12, 4.8), layout="constrained")
    grid = fig.add_gridspec(2, 2, height_ratios=[0.12, 1])
    legend = fig.add_subplot(grid[0, :])
    legend.axis("off")
    axes = [fig.add_subplot(grid[1, 0]), fig.add_subplot(grid[1, 1])]
    for index, summary in enumerate(summaries):
        rows = summary["conditions"]
        positions = [i + (index - 0.5) * 0.38 for i in range(len(rows))]
        for ax, key, digits in [(axes[0], "success_percent", 1), (axes[1], "rss_peak_mib", 0)]:
            bars = ax.bar(positions, [row[key] for row in rows], width=0.36, label=summary["environment"]["label"].capitalize(), color=colors[index])
            ax.bar_label(bars, fmt=f"%.{digits}f", fontsize=8, padding=3)
    for ax in axes:
        ax.set_xticks(range(len(summaries[0]["conditions"])), [str(row["limit"]) for row in summaries[0]["conditions"]])
        ax.set_xlabel("Buffered body slots per pod")
        ax.spines[["top", "right"]].set_visible(False)
        ax.grid(axis="y", alpha=0.18)
        ax.set_axisbelow(True)
        ax.set_ylim(bottom=0)
    axes[0].set_title("Accepted requests")
    axes[0].set_ylabel("Percent of offered requests")
    axes[0].set_ylim(0, 114)
    handles, labels = axes[0].get_legend_handles_labels()
    legend.legend(handles, labels, frameon=False, loc="center", ncol=2)
    axes[1].set_title("Worst observed Gateway RSS")
    axes[1].set_ylabel("MiB per individual pod")
    axes[1].set_ylim(0, max(row["rss_peak_mib"] for summary in summaries for row in summary["conditions"]) * 1.15)
    fig.suptitle("Two Gateway pods · 5 MiB JSON · 12 requests/s · 12s mock delay", fontsize=13)
    fig.supxlabel("Three fresh-pod repetitions per setting. RSS is the sampled maximum across six pod instances.", fontsize=9)
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(output, metadata={"Description": "Measured admission success and per-pod RSS; latency is reported separately because the mock adds a fixed twelve-second delay."})
    plt.close(fig)


if __name__ == "__main__":
    main()
