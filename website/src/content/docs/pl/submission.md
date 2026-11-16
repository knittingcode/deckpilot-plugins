---
title: "Zgłaszanie wtyczki"
description: "Zgłoszenie do rejestru wtyczek przez pull request: rekord wydawcy, wpis wtyczki, przepis budowania, rekord wersji, kontrole CI, przegląd i publikacja."
group: ship
order: 15
---

Wtyczki zgłasza się do repozytorium rejestru, [knittingcode/deckpilot-plugins](https://github.com/knittingcode/deckpilot-plugins), przez pull request. Rejestr nigdy nie przechowuje twojego kodu źródłowego: zapisuje, który tag i commit twojego własnego repozytorium zbudować, sam buduje pakiet ponownie, a właściciel repozytorium przeprowadza przegląd i publikuje.

> **Status:** Rejestr nie jest jeszcze otwarty na publikacje. Klucze produkcyjne nie istnieją, więc niczego nie można opublikować i żadna wtyczka nie została opublikowana. Zgłoszenie możesz już przygotować i zwalidować; według przewodnika dla współtwórców rejestru zostanie ono przejrzane i opublikowane, gdy rejestr zostanie otwarty.

## Zanim zaczniesz

- Twoja wtyczka jest modułem WebAssembly implementującym ABI wtyczek w wersji 1, zbudowanym za pomocą SDK dla Rusta.
- Twój kod źródłowy znajduje się w twoim własnym publicznym repozytorium GitHub.
- Twoja wtyczka ma licencję zadeklarowaną jako wyrażenie SPDX w `manifest.json` i zawiera plik `LICENSE`. Wtyczka zachowuje własną licencję; licencja Apache-2.0 repozytorium rejestru jej nie obejmuje.

### Przestrzenie nazw

Identyfikator wtyczki ma postać `<namespace>.<name>`. Przestrzeń nazw należy do ciebie od chwili scalenia twojego rekordu wydawcy i jest powiązana z twoim liczbowym identyfikatorem użytkownika GitHub.

Niektóre identyfikatory wydawców są zastrzeżone i nie mogą być przestrzenią nazw: `deckpilot` (własne wtyczki DeckPilota), `knittingcode`, nazwy ogólne (`core`, `system`, `plugin`, `plugins`, `builtin`, `dev`, `local`, `test`, `user`) oraz identyfikatory wbudowanych modułów (`general`, `projects`, `git`, `docker`, `http`, `notifications`, `github`, `gitlab`, `jira`, `workspace`, `work-session`). Kontrole rejestru je odrzucają, a DeckPilot odrzuca manifesty, które ich używają.

## Zgłaszanie nowej wtyczki

### 1. Otaguj kod źródłowy

Zacommituj `Cargo.lock`, przypnij toolchain Rusta w `rust-toolchain.toml` do dokładnej wersji i wypchnij tag, na przykład `v1.0.0`. Nigdy nie przesuwaj ani nie usuwaj tagu, który wskazuje rekord w rejestrze.

### 2. Dodaj pliki zgłoszenia

Zrób fork repozytorium rejestru i dodaj te pliki:

| Plik | Kiedy | Co |
| --- | --- | --- |
| `registry/publishers/<your-login>.json` | Tylko przy pierwszej wtyczce | Twój rekord wydawcy |
| `registry/plugins/<id>/plugin.json` | Pierwsza wersja; później, gdy zmienia się wpis wtyczki | Wpis wtyczki pokazywany w aplikacji |
| `plugins/<id>/build.json` | Pierwsza wersja; później, gdy zmienia się sposób budowania | Przepis budowania; zobacz [Pakowanie](../packaging/#przepis-budowania) |
| `plugins/<id>/manifest.json` | Każda wersja | Kopia `manifest.json` tej wersji, aby uprawnienia były widoczne w diffie pull requesta |
| `registry/plugins/<id>/versions/<version>.json` | Każda wersja | Rekord wersji |

Rekord wydawcy:

```json
{ "namespace": "acme", "github_id": 1001, "login": "acme", "verified": false }
```

`github_id` to twój liczbowy identyfikator GitHub; wypisuje go polecenie `gh api users/<login> -q .id`. Pozostaw `verified` z wartością `false`. Zmienia ją wyłącznie właściciel repozytorium.

Wpis wtyczki ma wymagane pola `id`, `name`, `summary`, `description` i `license` oraz opcjonalne pola `homepage`, `source_repository`, `categories` i `deprecated`. Oto wpis wtyczki będącej własnym zestawem testowym CI rejestru:

```json
{
  "id": "acme.hello-weather",
  "name": "Hello Weather",
  "summary": "Example plugin: a greeting and the current temperature of a city.",
  "description": "DeckPilot's sample plugin, built from the DeckPilot repository. It is a CI fixture of this registry and is never published.",
  "license": "Apache-2.0",
  "homepage": "https://github.com/knittingcode/deckpilot/tree/main/plugins/examples/hello-weather",
  "source_repository": "knittingcode/deckpilot",
  "categories": ["other"]
}
```

### 3. Wygeneruj rekord wersji

Rekord wersji to wynik działania narzędzia rejestru dla pakietu zbudowanego z twojego otagowanego kodu źródłowego. Zapisuje wersję, sha256 i rozmiar pakietu, skrót manifestu, uprawnienia, pole engines oraz repozytorium źródłowe, tag i commit. Utwórz go przypiętymi narzędziami w swoim forku repozytorium rejestru:

```sh
tools/build-cli.sh
tools/build-plugin.sh --recipe plugins/<id>/build.json \
  --repository <owner>/<repo> --commit <40-hex sha> --tag <tag> \
  --out .work/<id>.dpplugin
.work/bin/deckpilot-registry add-version --dir . --package .work/<id>.dpplugin \
  --plugin-detail registry/plugins/<id>/plugin.json \
  --source-repository <owner>/<repo> --tag <tag> --commit <sha> \
  --builder 1.0.0
rm -rf releases   # add-version also stores the asset here; never commit it
```

Jeśli wtyczka nie znajduje się w katalogu głównym twojego repozytorium, dodaj `--path <dir>` do `build-plugin.sh` oraz `--source-path <dir>` do `add-version`.

Buildem referencyjnym jest build CI rejestru na `ubuntu-24.04`, x86_64. Build na innym systemie operacyjnym lub innej architekturze może dać inny moduł, a więc inny sha256. Jeśli twój rekord się nie zgadza, zadanie *Validate registry* kończy się niepowodzeniem i wypisuje rekord wyprowadzony z własnego buildu. Zacommituj ten rekord.

### 4. Uruchom kontrole i otwórz pull request

```sh
tools/build-cli.sh
tools/validate.sh
```

`tools/validate.sh` buduje kod wtyczek ze źródeł, więc uruchamiaj go tylko dla wtyczek, które chcesz budować na swojej maszynie. Następnie otwórz pull request z szablonem repozytorium i wypełnij listę kontrolną zgłoszenia.

## Zgłaszanie aktualizacji

Dodaj nowy plik `registry/plugins/<id>/versions/<new-version>.json`, zastąp `plugins/<id>/manifest.json` nowym manifestem i w razie potrzeby zmień `plugin.json` lub `build.json`.

- Nowa wersja musi być wyższa od każdej zapisanej.
- Zapisane wersje są niezmienne. Nigdy żadnej nie edytuj ani nie usuwaj.
- Jeśli aktualizacja dodaje uprawnienia, wyjaśnij w pull requeście dlaczego. Użytkownicy są proszeni o zatwierdzenie dodanych uprawnień, zanim aktualizacja zostanie zastosowana.

## Co możesz zmieniać

| Ścieżka | Kto |
| --- | --- |
| `registry/publishers/<your-login>.json` | Ty, z własnym `github_id` i `login` |
| `registry/plugins/<your-namespace>.*/**`, `plugins/<your-namespace>.*/**` | Ty |
| `docs/*.md`, `README.md`, `plugins/README.md`, `registry/README.md` | Każdy |
| Workflow, `tools/`, `schemas/`, `keys/`, `registry/revocations.json`, pliki innych wydawców, dokumenty zasad | Tylko właściciel repozytorium |

## Co sprawdza CI

Wszystkie kontrole działają bez sekretów i z tokenem tylko do odczytu, także dla forków.

| Kontrola | Co robi |
| --- | --- |
| *Repository hygiene* | Brak kluczy prywatnych, plików kluczy, pakietów, kodu bajtowego Pythona i wygenerowanych plików witryny |
| *Submission rules* | Powyższa ochrona ścieżek, tożsamość wydawcy, brak zastrzeżonego identyfikatora wydawcy, wersje wyłącznie dopisywane. Różnica uprawnień względem poprzedniej wersji i link do porównania kodu źródłowego trafiają do podsumowania zadania |
| *Schemas match DeckPilot* | Schematy rejestru są równe schematom z commita DeckPilota, który rejestr przypina |
| *Validate registry* | Schematy wszystkich danych wejściowych; twój kod źródłowy pobrany w zapisanym commicie, na który musi wskazywać tag; `cargo build --locked`; `deckpilot-registry package`; rekord wersji wyprowadzony ponownie i porównany bajt po bajcie; cały rejestr podpisany jednorazowymi kluczami i zaakceptowany przez własnego klienta DeckPilota |
| *Build examples* | To samo dla przykładowych zestawów testowych rejestru |
| *actionlint and zizmor* | Lint workflow |

CI buduje twój kod, więc twoje skrypty budowania i makra proceduralne działają w zadaniu CI. To zadanie nie ma żadnych sekretów. Zadbaj o hermetyczność buildu.

> **Uwaga:** Rejestr buduje swoje narzędzia z jednego przypiętego commita DeckPilota, zapisanego w `tools/deckpilot.env`. Manifest używający pola dodanego do DeckPilota po tym commicie nie przejdzie tam walidacji, dopóki przypięcie nie zostanie przesunięte.

## Przegląd

Każde zgłoszenie jest przeglądane przez właściciela repozytorium, zanim zostanie scalone. Nic nie jest scalane automatycznie, a pomyślny przebieg CI nie jest zatwierdzeniem. Recenzent sprawdza:

- **Tożsamość wydawcy i wpis wtyczki.** Konto istnieje, jego liczbowy identyfikator się zgadza, przestrzeń nazw nie wprowadza w błąd, a wpis jest rzetelny.
- **Kod źródłowy.** Repozytorium i tag należą do ciebie, a różnica w kodzie źródłowym od poprzedniej wersji nie zawiera niczego niewyjaśnionego: zaciemniania kodu, wygenerowanych blobów, skryptów budowania, które cokolwiek pobierają lub wykonują, dołączonych plików binarnych.
- **Uprawnienia.** Każde uzasadnienie zgadza się z tym, co wtyczka robi, hosty sieciowe należą do usługi, z której wtyczka deklaruje korzystanie, pola danych uwierzytelniających (sloty) odpowiadają tym hostom, a hosty `openUrl` to strony, które akcje wtyczki otwierają dla użytkownika.
- **Build.** Dokładna wersja toolchainu, workspace Cargo z plikiem lock i zwykłe `cargo build` jednego pakietu; pakiet został ponownie zbudowany przez CI i zgadza się z rekordem bajt po bajcie.
- **Licencja.** Manifest deklaruje licencję SPDX, a pakiet zawiera swój plik `LICENSE`.

Nie ma gwarantowanych czasów odpowiedzi.

## Scalenie to nie publikacja

Scalony pull request niczego nie publikuje. Właściciel repozytorium publikuje wersję, tworząc jej tag wydania, `<id>--v<version>`, na scalonym commicie. Uruchamia to proces publikowania, który ponownie buduje pakiet, podpisuje nowy indeks i znacznik czasu, tworzy wydanie GitHub Release z pakietem i wdraża metadane. Ten proces nie był dotąd uruchamiany z kluczami produkcyjnymi.

Współtwórcy nigdy nie otrzymują praw do publikowania: ani przez scalenie ich wtyczki, ani przez status zweryfikowanego wydawcy, ani przez regularny wkład. Tagi, wydania, środowiska, klucze i sekrety należą do właściciela repozytorium. Odznaka zweryfikowanego wydawcy jest decyzją właściciela repozytorium podjętą w ramach przeglądu, a nie uprawnieniem.

## Zgłaszanie problemów

Błędy w narzędziach rejestru zgłasza się w issue typu *Bug report*, a wtyczkę, która działa nieprawidłowo lub wprowadza w błąd, w issue typu *Plugin report*. Wszystko, co złośliwe, oraz luki zgłasza się prywatnie; zobacz [Zasady bezpieczeństwa](../security/#zgłaszanie-luki).

## Dokumentacja źródłowa

[CONTRIBUTING.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/CONTRIBUTING.md), [docs/publishing.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/publishing.md) i [docs/review-policy.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/review-policy.md) w repozytorium rejestru.
