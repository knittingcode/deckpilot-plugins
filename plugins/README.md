# Plugin submissions

One directory per plugin, named by its id (`<publisher>.<name>`):

| File | What |
| --- | --- |
| `<id>/build.json` | Build recipe: Rust toolchain, Cargo workspace, package, profile, module file name (`schemas/inputs/build-recipe.schema.json`) |
| `<id>/manifest.json` | Copy of the newest version's `manifest.json`, so the permissions are visible in the pull request diff; CI checks it equals the built package's |
| `<id>/README.md` | Optional notes for reviewers |

The source itself stays in the author's repository, pinned per version by
`registry/plugins/<id>/versions/<version>.json` (`source.repository`,
`source.tag`, `source.commit`, `source.path`). See
[CONTRIBUTING.md](../CONTRIBUTING.md).

`examples/` holds CI fixtures that are built and verified in a throwaway
registry on every run and are **never published**.
