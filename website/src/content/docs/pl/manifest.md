---
title: "Format manifestu"
description: "Każde pole pliku manifest.json w wersji manifestu 1, reguły stosowane przez JSON Schema i agenta oraz kompletny, poprawny przykład."
group: build
order: 4
---

`manifest.json` leży w katalogu głównym źródeł wtyczki i jej pakietu. To jedyna rzecz, którą DeckPilot czyta, żeby dowiedzieć się, czym jest wtyczka, o co prosi i co oferuje. Agent nigdy nie uruchamia kodu wtyczki, żeby poznać metadane, więc manifest i kod muszą być ze sobą zgodne.

Poniższe reguły pochodzą z JSON Schema (draft 2020-12) w pliku [plugins/schemas/plugin-manifest.schema.json](https://github.com/knittingcode/deckpilot/blob/main/plugins/schemas/plugin-manifest.schema.json) oraz z walidacji w cracie `deckpilot-plugin-manifest`, która dodaje kontrole obejmujące wiele pól naraz. Nieznane pola są wszędzie odrzucane, a manifest ma co najwyżej 256 KiB.

## Pola najwyższego poziomu

| Pole | Wymagane | Reguła |
| --- | --- | --- |
| `$schema` | nie | Dowolny ciąg znaków o długości co najwyżej 256 znaków. Przykłady używają `https://knittingcode.github.io/deckpilot-plugins/v1/schemas/plugin-manifest.schema.json` |
| `manifestVersion` | tak | `1` |
| `id` | tak | `<publisher>.<name>`. Każda część pasuje do `[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*`; wydawca ma od 2 do 24 znaków, nazwa od 2 do 32. Musi zaczynać się od `publisher.id` i kropki |
| `name` | tak | Nazwa wyświetlana, od 1 do 48 znaków |
| `version` | tak | `X.Y.Z` z opcjonalnym `-alpha.N`, `-beta.N` lub `-rc.N` (N co najmniej 1). Bez metadanych builda, bez zer wiodących. Musi być równa wersji w `Plugin::new` |
| `publisher` | tak | `{ "id", "name", "url"? }`. `id` nie może być zastrzeżone; zobacz [Zgłaszanie wtyczki](../submission/). `name` ma od 1 do 64 znaków, `url` to adres URL https |
| `description` | tak | Od 1 do 300 znaków |
| `homepage` | nie | Adres URL https o długości co najwyżej 512 znaków |
| `repository` | nie | `{ "type": "git", "url": "https://github.com/<owner>/<repo>", "directory"? }` |
| `license` | tak | Wyrażenie SPDX, od 1 do 64 znaków. Pakiet musi zawierać plik `LICENSE` |
| `keywords` | nie | Co najwyżej 10 unikalnych słów kluczowych, każde w postaci `[a-z0-9-]{2,24}` |
| `categories` | nie | Co najwyżej 3 unikalne wartości spośród `developer-tools`, `productivity`, `communication`, `monitoring`, `media`, `smart-home`, `other` |
| `engines` | tak | Wymagania dotyczące wersji; zobacz [Zgodność wersji](../compatibility/) |
| `runtime` | w praktyce tak | `{ "kind": "wasm", "abi": 1 }`. W schemacie opcjonalne, ale pakiet bez tego pola jest odrzucany |
| `entry` | tak | Informacyjna ścieżka względna do punktu wejścia źródeł, na przykład `src/lib.rs`. Agent ją ignoruje |
| `platforms` | nie | Niepusta lista unikalnych wartości spośród `macos`, `windows`, `linux`. Pominięcie oznacza wszystkie trzy |
| `icons` | tak | `{ "128": "assets/….png" }` jest wymagane; `32` i `256` są opcjonalne. Agent pokazuje ikonę 128 pikseli tylko wtedy, gdy jest to plik PNG o rozmiarze co najwyżej 256 KiB |
| `assets` | nie | `keyIcons` (co najwyżej 64 pliki PNG według nazwy), `screenshots` (co najwyżej 8 plików PNG), `readme` (`"README.md"`), `changelog` (`"CHANGELOG.md"`). Wszystko, co tu wymieniono, trafia do pakietu |
| `permissions` | tak | Tablica o co najwyżej 16 pozycjach; może być pusta. Zobacz [Uprawnienia](../permissions/) |
| `supports` | nie | Rodziny urządzeń, których wtyczka wymaga lub których używa. Akceptowane przez schemat, nieużywane przez agenta w tej wersji |
| `contributes` | tak | `actions`, `variables`, `settings`, `credentials`, `errorCodes`, `navigation`, `views`; każde z nich jest opcjonalne |

## Akcje

`contributes.actions` zawiera co najwyżej 64 akcje. Agent rejestruje każdą jako typ akcji `<plugin id>.<action id>`.

| Pole | Wymagane | Reguła |
| --- | --- | --- |
| `id` | tak | `[a-z][a-z0-9]*(-[a-z0-9]+)*`, od 1 do 48 znaków, unikalne. Musi być równe identyfikatorowi w `Action::new` |
| `name` | tak | Od 1 do 48 znaków |
| `description` | tak | Od 1 do 300 znaków |
| `category` | tak | Od 1 do 32 znaków: grupa na listach wyboru w edytorach |
| `parameters` | tak | Co najwyżej 32; może być puste. Zobacz [Parametry i ustawienia](../parameters/) |
| `node` | nie | Kontrakt węzła workflow: wyjścia, limit czasu, ponawianie i efekt. Zobacz [Akcje](../actions/#metadane-węzła) |
| `requiresProject`, `deprecated`, `replacedBy` | nie | `replacedBy` wskazuje inną akcję tej wtyczki i wymaga `deprecated: true` |
| `state` | nie | Odświeżanie stanu klawisza (`onEvents` albo `every` z `intervalSeconds` od 1 do 86 400; wymaga `effect: readOnly`). Od API wtyczek 1.2 agent pyta procedurę stanu akcji o klawisze widoczne na urządzeniu, najwyżej co 5 sekund; wymaga to uprawnienia `device` z `state: true`. Zobacz [stan klawisza](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/actions.md#key-state) w dokumentacji DeckPilota |
| `device` | nie | `{ "requires": [...] }`. Akceptowane, nieużywane: nigdy nie pozwala przypisać akcji do pokrętła ani strefy dotykowej; do tego służy `controls` |
| `controls` | nie | API wtyczek 1.3: kontrolki i wejścia, na których działa akcja. Bez tego pola tylko klawisze. Zobacz [Kontrolki](#kontrolki) |

### Kontrolki

`controls` wymaga `"engines": { "pluginApi": "^1.3" }` oraz, aby DeckPilot 0.7.0 nie oferował wtyczki, `"deckpilot": ">=0.8.0"`. Dla każdego rodzaju kontrolki pole wymienia wejścia, do których można przypisać akcję:

```json
"controls": { "key": ["press", "click", "long-press"],
              "encoder": ["rotate-clockwise", "rotate-counter-clockwise"] }
```

| Rodzaj | Wejścia, które może wymienić |
| --- | --- |
| `key` | `press`, `release`, `click`, `double-click`, `long-press` |
| `encoder` (pokrętło) | `rotate-clockwise`, `rotate-counter-clockwise`, `press`, `release`, `click`, `double-click`, `long-press` (pięć ostatnich tylko na pokrętłach, które można wcisnąć) |
| `touchZone` | `tap`, `long-touch`, `swipe` |

- Co najmniej jeden rodzaj; każdy rodzaj wymienia od 1 do 8 wejść, każde raz, i tylko wejścia, które kontrolka tego rodzaju może wytworzyć.
- Rodzaj, którego brak, nie jest obsługiwany: edytory nie oferują tam akcji, zapis takiego przypisania jest odrzucany, a agent nie uruchamia przypisania zapisanego wcześniej. DeckPilot 0.8.0 zachowuje takie przypisanie i pokazuje je jako niedostępne.
- Bez `controls` akcja działa tylko na klawiszach, ze wszystkimi wejściami klawisza: to domyślne zachowanie każdego manifestu sprzed 1.3. `device.requires`, nawet `["dials"]`, nie dodaje żadnego rodzaju.
- Akcja pokrętła uruchamia się raz na każde zdarzenie obrotu zgłoszone przez urządzenie, niezależnie od liczby ząbków.
- W SDK akcja deklaruje to samo za pomocą `Action::controls`; zobacz [Przegląd SDK](../sdk/#kontrolki).

Pełne reguły opisują [dokumentacja manifestu w DeckPilocie](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md#controls) i [docs/device-controls.md](https://github.com/knittingcode/deckpilot/blob/main/docs/device-controls.md).

## Zmienne

`contributes.variables` deklaruje co najwyżej 64 zmienne, które wtyczka może publikować. Zobacz [Zmienne](../variables/).

| Pole | Reguła |
| --- | --- |
| `id` | Ścieżka względna: `[a-z][A-Za-z0-9_]{0,31}` oraz maksymalnie dwa kolejne segmenty oddzielone kropkami, co najwyżej 100 znaków, unikalna |
| `name`, `description` | Od 1 do 48 znaków; co najwyżej 300 |
| `valueType` | `{ "type": "string" }`, `integer`, `boolean` lub `json` |
| `scope` | `machine`, `profile` lub `project`. W tej wersji można publikować tylko `machine` |
| `requires`, `sensitive`, `deprecated` | Opcjonalne |

## Ustawienia

`contributes.settings` ma postać `{ "portabilityVersion"?, "fields": [...] }` i zawiera co najwyżej 32 pola. Pole ma te same `key`, `label`, `description`, `kind`, `required` i `default` co parametr akcji, a do tego `portability`: `portable`, `machineSpecific` (wartość domyślna) albo `sensitive`. Klucz `privateHosts` jest zastrzeżony. Ustawienie może zawierać adres serwera wpisany przez użytkownika (wskazany przez slot `network.userOrigins`, zobacz [Uprawnienia](../permissions/#adresy-serwerów-podane-przez-użytkownika)) albo decydować, gdzie pojawiają się powiadomienia (wskazane przez `notifications.routing`, zobacz [Uprawnienia](../permissions/#kierowanie-powiadomień)). Nie ma ustawień będących sekretami; sekrety to pola danych uwierzytelniających (sloty). Zobacz [Parametry i ustawienia](../parameters/#ustawienia-wtyczki).

## Dane uwierzytelniające

`contributes.credentials` deklaruje co najwyżej 8 pól danych uwierzytelniających (slotów). Zobacz [Dane uwierzytelniające](../credentials/).

| Pole | Reguła |
| --- | --- |
| `key` | camelCase, unikalny. Musi być wymieniony w `refs` uprawnienia `credentials`, a każde odwołanie musi być slotem |
| `label`, `description` | Od 1 do 48 znaków; co najwyżej 300 |
| `hosts` | Od 1 do 8 dokładnych hostów, z których każdy jest objęty polem `hosts` uprawnienia `network`. Może być puste tylko razem z `userOrigin` |
| `userOrigin` | Ustawienie slotu `network.userOrigins`; zobacz [Uprawnienia](../permissions/#adresy-serwerów-podane-przez-użytkownika) |
| `schemes` | Niepusty podzbiór `bearer`, `basic`, `header`, `query` |
| `headerName` | Wymagane przy `header`: `[A-Za-z0-9-]{1,64}` |
| `queryName` | Wymagane przy `query`: `[A-Za-z0-9_.-]{1,64}` |
| `setupUrl` | Adres URL https, pod którym użytkownik tworzy sekret |

## Pozycje paska narzędzi i widoki

`contributes.navigation` i `contributes.views` wymagają `"engines": { "pluginApi": "^1.3" }` oraz, aby DeckPilot 0.7.0 nie oferował wtyczki, `"deckpilot": ">=0.8.0"`. Każda pozycja nawigacji to przycisk na pasku narzędzi głównego okna DeckPilota, który otwiera widok rysowany przez samego DeckPilota. Wtyczka nie dostarcza do niego HTML-a, adresu URL ani kodu.

```json
"navigation": [{ "id": "dashboard", "label": "Status",
                 "icon": { "light": "assets/nav.svg", "dark": "assets/nav-dark.svg" },
                 "view": "dashboard" }],
"views": [{ "id": "dashboard", "kind": "variables" }]
```

| Pole | Reguła |
| --- | --- |
| `navigation` | Co najwyżej 4 pozycje, unikalne identyfikatory |
| `navigation[].id` | `[a-z][a-z0-9]*(-[a-z0-9]+)*`, od 1 do 32 znaków |
| `navigation[].label` | Od 1 do 24 znaków, bez spacji na początku i na końcu, bez znaków sterujących |
| `navigation[].icon` | Jedna ścieżka dla obu motywów albo `{ "light", "dark" }` z co najmniej jedną z nich |
| `navigation[].view` | Identyfikator jednego z `views` |
| `views` | Co najwyżej 8 widoków, unikalne identyfikatory; `id` jak `navigation[].id` |
| `views[].kind` | `variables` (zmienne wtyczki, na żywo), `actions` (Action Library z akcjami wtyczki) lub `settings` (ustawienia wtyczki) |

Ikona to plik pakietu w katalogu `assets/`, z rozszerzeniem `.png`, `.webp` lub `.svg`. Jest sprawdzana przy budowaniu pakietu i przy instalacji: PNG lub WebP o rozmiarze najwyżej 256 KiB i 512×512 pikseli albo SVG o rozmiarze najwyżej 64 KiB, bez skryptów, `<foreignObject>`, procedur obsługi zdarzeń, linków zewnętrznych, `data:` i `javascript:`, importów stylów i encji. Agent ponownie oczyszcza SVG za każdym razem, gdy je udostępnia, a aplikacja pokazuje ikony wyłącznie jako obrazy, z własną ikoną, gdy nie ma czego pokazać. Pozycje pojawiają się tylko dla zainstalowanych i włączonych wtyczek; te, które się nie mieszczą, są w menu *More plugin views*.

Pełne reguły dotyczące ikon opisuje [dokumentacja manifestu w DeckPilocie](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md#contributesnavigation-and-contributesviews).

## Kody błędów

`contributes.errorCodes` przypisuje co najwyżej 64 kodom ich znaczenie o długości od 1 do 200 znaków. Kod pasuje do `[a-z][a-z0-9]*(-[a-z0-9]+)*`, ma co najwyżej 64 znaki i nie może ponownie deklarować kodu wspólnego. Zobacz [Akcje](../actions/#kody-błędów).

## Moduł musi być zgodny z manifestem

Trzy kontrole pilnują zgodności manifestu z kodem:

- **Identyfikatory akcji.** Kiedy proces hosta ładuje moduł, nadzorca porównuje identyfikatory akcji zgłaszane przez moduł z identyfikatorami w manifeście i odrzuca niezgodność.
- **Parametry.** Agent waliduje każde wywołanie względem manifestu, zanim uruchomi się proces hosta, a SDK waliduje je ponownie względem deklaracji `Param`.
- **Test jednostkowy.** `testing::manifest_mismatches` wymienia każdą różnicę w identyfikatorze, wersji, `engines.pluginApi`, `engines.sdk`, identyfikatorach akcji i ich kolejności, w `controls` akcji oraz w `required`, rodzaju, ograniczeniach, opcjach i wartości domyślnej każdego parametru. Zobacz [Testowanie](../testing/).

## Pełny przykład

Ten manifest należy do przykładu używanego na wszystkich tych stronach: wtyczki, która sprawdza usługę w API statusu. Używa czterech rodzajów uprawnień, czterech akcji z wyjściami, dwóch zmiennych, jednego ustawienia, jednego pola danych uwierzytelniających i pięciu kodów błędów. Przechodzi walidację względem schematu i został zaakceptowany przez `deckpilot-registry package` razem ze swoim modułem.

```json
{
  "$schema": "https://knittingcode.github.io/deckpilot-plugins/v1/schemas/plugin-manifest.schema.json",
  "manifestVersion": 1,
  "id": "acme.status-check",
  "name": "Status Check",
  "version": "0.1.0",
  "publisher": { "id": "acme", "name": "Acme Tools", "url": "https://acme.example" },
  "description": "Checks whether a service is up on the Acme status API and remembers the last result.",
  "repository": { "type": "git", "url": "https://github.com/acme/status-check" },
  "license": "Apache-2.0",
  "keywords": ["status", "monitoring"],
  "categories": ["monitoring"],
  "engines": { "deckpilot": ">=0.5.0 <1.0.0", "pluginApi": "^1.0", "sdk": "0.1.0" },
  "runtime": { "kind": "wasm", "abi": 1 },
  "entry": "src/lib.rs",
  "icons": { "128": "assets/icon-128.png" },
  "permissions": [
    {
      "kind": "network",
      "hosts": ["status.example.com"],
      "methods": ["GET"],
      "justification": "Reads the status of the service you name from the Acme status API."
    },
    {
      "kind": "credentials",
      "refs": ["apiToken"],
      "justification": "Sends your Acme API token to status.example.com to authenticate the status request."
    },
    {
      "kind": "notifications",
      "justification": "Tells you when the status of a service changes, only if the action asks for it."
    },
    {
      "kind": "openUrl",
      "hosts": ["status.example.com"],
      "justification": "Opens the public status page of a service in your browser when you run Open status page."
    }
  ],
  "contributes": {
    "actions": [
      {
        "id": "check",
        "name": "Check service",
        "description": "Reads the current status of a service and publishes it as a variable.",
        "category": "Status Check",
        "parameters": [
          {
            "key": "service",
            "label": "Service",
            "description": "The service to check. Empty uses the default service from the settings.",
            "required": false,
            "kind": { "type": "text", "maxLength": 64 }
          },
          {
            "key": "minUptime",
            "label": "Minimum uptime (%)",
            "required": false,
            "kind": { "type": "integer", "min": 0, "max": 100 },
            "default": { "type": "integer", "value": 99 }
          },
          {
            "key": "notify",
            "label": "Notify on change",
            "required": false,
            "kind": { "type": "boolean" },
            "default": { "type": "boolean", "value": false }
          }
        ],
        "node": {
          "version": 1,
          "icon": "activity",
          "outputs": [
            { "key": "status", "label": "Status", "kind": "text", "always": true },
            { "key": "uptime", "label": "Uptime (%)", "kind": "integer", "always": true },
            { "key": "healthy", "label": "Healthy", "kind": "boolean", "always": true },
            { "key": "checkedAt", "label": "Checked at", "kind": "text" }
          ],
          "timeoutMs": 20000,
          "retrySafe": true,
          "idempotent": true,
          "effect": "readOnly"
        }
      },
      {
        "id": "last-check",
        "name": "Last check",
        "description": "Shows the result of the last check from the plugin's storage, without a request.",
        "category": "Status Check",
        "parameters": [],
        "node": {
          "version": 1,
          "outputs": [
            { "key": "service", "label": "Service", "kind": "text" },
            { "key": "status", "label": "Status", "kind": "text" },
            { "key": "checkedAt", "label": "Checked at", "kind": "text" }
          ],
          "timeoutMs": 5000,
          "retrySafe": true,
          "idempotent": true,
          "effect": "readOnly"
        }
      },
      {
        "id": "open-page",
        "name": "Open status page",
        "description": "Opens the status page of a service in the browser.",
        "category": "Status Check",
        "parameters": [
          {
            "key": "service",
            "label": "Service",
            "required": true,
            "kind": { "type": "text", "maxLength": 64 }
          }
        ],
        "node": {
          "version": 1,
          "outputs": [{ "key": "url", "label": "Page", "kind": "url", "always": true }],
          "timeoutMs": 5000,
          "effect": "opens"
        }
      },
      {
        "id": "reset",
        "name": "Forget last check",
        "description": "Deletes the stored result and clears the published variables.",
        "category": "Status Check",
        "parameters": [],
        "node": { "version": 1, "timeoutMs": 5000, "idempotent": true, "effect": "modifies" }
      }
    ],
    "variables": [
      {
        "id": "status",
        "name": "Service status",
        "description": "The status the last check found, for example up or degraded.",
        "valueType": { "type": "string" },
        "scope": "machine"
      },
      {
        "id": "healthy",
        "name": "Service healthy",
        "valueType": { "type": "boolean" },
        "scope": "machine"
      }
    ],
    "settings": {
      "fields": [
        {
          "key": "defaultService",
          "label": "Default service",
          "description": "Checked when an action names no service.",
          "required": false,
          "kind": { "type": "text", "maxLength": 64 },
          "portability": "portable"
        }
      ]
    },
    "credentials": [
      {
        "key": "apiToken",
        "label": "Acme API token",
        "description": "A read-only token of the Acme status API.",
        "hosts": ["status.example.com"],
        "schemes": ["bearer"],
        "setupUrl": "https://status.example.com/account/tokens"
      }
    ],
    "errorCodes": {
      "service-required": "No service was named and the settings have no default service.",
      "invalid-service": "The service name contains characters the status API does not use.",
      "service-not-found": "The status API does not know this service.",
      "status-api-error": "The status API answered with an error status.",
      "invalid-response": "The status API answered with something that is not a status."
    }
  }
}
```

Host `status.example.com` jest wartością zastępczą. Prawdziwe manifesty trzech przykładowych wtyczek znajdują się w repozytorium DeckPilota; [Developer Tools](https://github.com/knittingcode/deckpilot/blob/main/plugins/examples/developer-tools/manifest.json) używa każdej sekcji.

## Dokumentacja źródłowa

Wyczerpujący opis pól to [docs/marketplace/sdk/manifest.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md).
