# Development workflow

`main` holds the initial stable baseline. Integrate reviewed changes into
`develop` using a dedicated `fix/<description>`, `feature/<description>` or
`chore/<description>` branch and a merge commit that describes the resulting
behavior. Promote to `main` separately.

Before merging processing changes, run:

```sh
cargo test --release --locked -j4
cargo clippy --release --locked --all-targets -j4 -- -D warnings
```

Compare affected processing paths with the pinned experimental reference using
small fixtures or crops. Keep the reference checkout read-only; see `AGENTS.md`
and `VALIDATION.md` for the numerical contract.

For GUI changes, also run the release suite and Clippy with `--features gui`.
Optional Lensfun and metadata checks need explicit enablement; see
[VALIDATION.md](VALIDATION.md). There is currently no checked-in CI workflow to
run these gates automatically. Treat the open debts in [MIGRATION.md](MIGRATION.md)
as unclosed until their acceptance checks pass; a documentation audit does not
fix the underlying behavior.

## Repository hygiene

Commit source, dependency lockfiles, licenses, required measurement/LUT/profile
data, and small regression fixtures. Preserve copied data and upstream license
notices unchanged. NPY fixtures and LUTs and ICC profiles are required data,
not compiled executables.

Do not commit compiled binaries, build directories, RAW samples, virtual
environments, tool installations, caches, generated reports, rendered images,
credentials, or machine-specific absolute paths. Keep generated outputs under
`artifacts/` or `validation/`; both directories are ignored. Use relative paths
or command-line options in portable instructions and scripts.

Inspect `git diff --cached` and `git status --short` before committing. Check
the commit author/committer identity as well when keeping personal information
out of repository history.
