"""Render README charts from saved release benchmark and differential reports."""
from pathlib import Path
import argparse
import json

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
ORANGE, GREY, TEXT, BACKGROUND = '#ff985a', '#8fa3b8', '#eeeeee', '#111111'
MODES = ('deterministic', 'spatial', 'legacy-grain')
LABELS = ('No spatial effects', 'Spatial effects', 'Spatial + legacy grain')


def load_reports(folder):
    reports = [json.loads((folder / f'{mode}.json').read_text()) for mode in MODES]
    accuracy = json.loads((folder / 'accuracy.json').read_text())
    core = json.loads((folder / 'core.json').read_text())
    release = json.loads((folder / 'release.json').read_text())
    binary_hash = release['binary_sha256']
    for report in reports + [accuracy, core]:
        assert report['binary_sha256'] == binary_hash, 'Mixed processor binaries'
        assert report['baseline_commit'] == release['baseline_commit']
        for result in report['results']:
            comparison = result.get('comparison', result)
            assert comparison['valid'] and comparison['finite']
            assert comparison['failures'] == 0
    assert accuracy['atol'] == 1e-9 and accuracy['rtol'] == 1e-8
    for mode, report in zip(MODES, reports):
        assert report['mode'] == mode and report['film_format_mm'] == 35
        assert report['threads'] == 4 and report['repeats'] == 3
        assert len(report['results']) == 4
        assert all(r['shape'] == [256, 256, 3] for r in report['results'])
    assert len({tuple((r['crop'], r['input_sha256']) for r in p['results']) for p in reports}) == 1
    return reports, accuracy, core, release


def figure(width, height, title, subtitle):
    fig, ax = plt.subplots(figsize=(width, height))
    fig.patch.set_facecolor(BACKGROUND)
    ax.set_facecolor(BACKGROUND)
    fig.text(.04, .94, title, color=TEXT, size=19, weight='bold', va='top')
    fig.text(.04, .855, subtitle, color=GREY, size=10, va='top')
    ax.tick_params(colors=TEXT, labelsize=10, length=0, pad=8)
    for spine in ax.spines.values():
        spine.set_visible(False)
    ax.set_axisbelow(True)
    ax.grid(axis='x', color='#303030', linewidth=.7)
    return fig, ax


def save(fig, name, args):
    args.output_dir.mkdir(parents=True, exist_ok=True)
    svg = args.output_dir / f'{name}.svg'
    fig.savefig(svg, facecolor=BACKGROUND,
                metadata={'Date': None, 'Creator': 'tools/plot_release_report.py'})
    svg.write_text('\n'.join(line.rstrip() for line in svg.read_text().splitlines()) + '\n')
    if args.preview_dir:
        args.preview_dir.mkdir(parents=True, exist_ok=True)
        fig.savefig(args.preview_dir / f'{name}.png', dpi=160, facecolor=BACKGROUND)
    plt.close(fig)


def performance(reports, release, args, memory=False):
    title = 'Peak RAM' if memory else 'Processing time'
    fig, ax = figure(6.4, 4.7, title,
                    f"v{release['version']} · 256 × 256 crops · 4 threads")
    fig.subplots_adjust(left=.28, right=.98, bottom=.22, top=.69)
    all_values = []
    for key, label, colour, offset in (
        ('reference', 'spektrafilm (Python)', GREY, -.18),
        ('rust', 'fast-spektrafilm (Rust)', ORANGE, .18),
    ):
        values = np.array([
            [r[key]['peak_rss_kib'] / 1024 if memory else
             float(np.median(r[key]['process_seconds'])) * 1000
             for r in report['results']]
            for report in reports
        ])
        centers = np.median(values, axis=1)
        errors = np.array([centers - values.min(axis=1), values.max(axis=1) - centers])
        positions = np.arange(len(reports)) + offset
        ax.barh(positions, centers, height=.27, color=colour, label=label,
                xerr=errors, error_kw={'ecolor': TEXT, 'capsize': 3, 'elinewidth': 1})
        for y, value, high in zip(positions, centers, values.max(axis=1)):
            ax.annotate(f'{value:.1f}', (high, y), xytext=(7, 0),
                        textcoords='offset points', va='center', color=colour, size=10)
        all_values.extend(values.ravel())
    ax.set_xlim(0, max(all_values) * 1.24)
    ax.set_yticks(range(len(reports)), LABELS)
    ax.invert_yaxis()
    ax.set_xlabel('MiB · lower is better' if memory else 'ms / crop · lower is better',
                  color=GREY, labelpad=10, size=10)
    fig.legend(*ax.get_legend_handles_labels(), loc='upper left',
               bbox_to_anchor=(.04, .805), ncol=2, frameon=False,
               labelcolor=TEXT, fontsize=9, handlelength=1.3, columnspacing=1.4)
    footer = ('Process peak RSS includes startup, JIT and preparation.' if memory else
              'Render only; excludes decoding, setup, JIT and export.')
    fig.text(.04, .065, footer, color=GREY, size=9)
    fig.text(.04, .025, 'Median across 4 cameras; whiskers show the camera range.', color=GREY, size=9)
    save(fig, 'memory' if memory else 'speed', args)


def accuracy_chart(report, core, release, args):
    groups = (
        ('Input / RAW decode', ('decode', 'rgb_pre')),
        ('Film exposure', ('log_e_film',)),
        ('Film density', ('cmy_film',)),
        ('Print exposure', ('log_e_print',)),
        ('Print density', ('cmy_print',)),
        ('Output RGB', ('rgb_out',)),
    )
    total = len(report['results']) + len(core['results'])
    fig, ax = figure(12.8, 5.2, 'Agreement with spektrafilm',
                    f"v{release['version']} · {total} / {total} comparisons passed · float64 arrays")
    fig.subplots_adjust(left=.21, right=.80, bottom=.23, top=.75)
    rows = []
    for label, taps in groups:
        results = [r for r in report['results'] if r['case'].rsplit('/', 1)[-1] in taps]
        assert results, label
        rows.append((label, max(r['max_tolerance_ratio'] for r in results),
                     max(r['max_abs'] for r in results)))
    for mode, label in (('convert', 'Inverse → print → scan'), ('diffusion', 'Diffusion → print → scan')):
        result = next(r['comparison'] for r in core['results'] if r['mode'] == mode)
        rows.append((label, result['max_tolerance_ratio'], result['max_abs']))
    fig.text(.83, .77, 'Max absolute error', color=GREY, fontsize=10)
    for y, (_, ratio, error) in enumerate(rows):
        assert ratio > 0
        ax.plot([1e-8, ratio], [y, y], color='#454545', linewidth=2)
        ax.scatter(ratio, y, color=ORANGE, s=45, zorder=3)
        ax.text(1.05, y, f'{error:.2e}', transform=ax.get_yaxis_transform(),
                color=ORANGE, va='center', fontsize=11)
    ax.set_xscale('log')
    ax.set_xlim(1e-8, 3)
    ax.set_ylim(len(rows) - .6, -.5)
    ax.set_yticks(range(len(rows)), [label for label, _, _ in rows])
    ax.set_xticks([1e-8, 1e-6, 1e-4, 1e-2, 1], ['0.000001%', '0.0001%', '0.01%', '1%', '100%'])
    ax.axvline(1, color=GREY, linestyle='--', linewidth=1)
    ax.text(1, -.8, 'Acceptance limit', color=GREY, fontsize=10, ha='right')
    ax.set_xlabel('Allowed tolerance used · log scale · lower is better', color=GREY, labelpad=10)
    fig.text(.04, .07, 'Dots: worst pointwise error / tolerance. Right column: worst absolute error across each group.', color=GREY, size=10)
    fig.text(.04, .025, 'Strict tolerance: 1e−9 + 1e−8 × |reference|. Combined optics: 2e−6 absolute. Reference: 28bf883e.', color=GREY, size=10)
    save(fig, 'accuracy', args)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data-dir', type=Path, default=ROOT / 'docs/benchmarks/v0.1.2')
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'docs/images/benchmarks/v0.1.2')
    parser.add_argument('--preview-dir', type=Path)
    args = parser.parse_args()
    plt.rcParams.update({'font.family': 'DejaVu Sans', 'svg.fonttype': 'none',
                         'svg.hashsalt': 'fast-spektrafilm-release-charts'})
    reports, accuracy, core, release = load_reports(args.data_dir)
    performance(reports, release, args)
    performance(reports, release, args, memory=True)
    accuracy_chart(accuracy, core, release, args)


if __name__ == '__main__':
    main()
