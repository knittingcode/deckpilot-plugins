# Example: hello-weather (never published)

DeckPilot's sample plugin, built from
[`plugins/examples/hello-weather`](https://github.com/knittingcode/deckpilot/tree/dc1a8a2ee190df080377ab372c33eafb3c1f55a0/plugins/examples/hello-weather)
at the DeckPilot commit pinned in `source.json`.

`validate.yml` (job *Build examples*) builds it with `tools/build-plugin.sh`
from `build.json`, adds it to a registry made only of `registry/` here, signs
that registry with throwaway keys and runs `verify-site --require-packages`.
It shows the whole submission format working end to end. The `acme`
publisher (GitHub id 1001) is fictional; this example never enters the real
`registry/` and nothing here is released.
