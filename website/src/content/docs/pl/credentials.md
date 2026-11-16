---
title: "Dane uwierzytelniające"
description: "Pola danych uwierzytelniających i model oparty wyłącznie na uchwytach: wtyczka wskazuje slot, agent wstrzykuje sekret, a kod wtyczki nigdy go nie widzi."
group: capabilities
order: 9
---

Wiele wtyczek potrzebuje tokenu API. W DeckPilocie wtyczka nigdy go nie otrzymuje. Deklaruje pole danych uwierzytelniających (slot), użytkownik zapisuje w tym slocie sekret, a wtyczka, wysyłając żądanie, wskazuje slot. Agent wstrzykuje sekret do tego jednego żądania. Sekret nigdy nie trafia do pamięci modułu, do jego logów ani do jego wyników.

## Model oparty wyłącznie na uchwytach

`SecretHandle` to klucz slotu i nic więcej. Żądanie funkcji hosta (capability), które wysyła moduł, zawiera klucz i schemat; wykonawca (executor) w agencie dodaje sekret dopiero wtedy, gdy broker uprawnień zezwolił na żądanie.

| Krok | Gdzie | Co się dzieje |
| --- | --- | --- |
| Deklaracja | `manifest.json` | Slot ze swoimi hostami i schematami oraz uprawnienie `credentials`, które się do niego odwołuje |
| Zapisanie | Aplikacja | Użytkownik wpisuje sekret; jest on przechowywany w zaszyfrowanej bazie danych wtyczek agenta |
| Wskazanie | Kod wtyczki | `SecretHandle::slot("apiToken")` i `Request::bearer(&handle)` |
| Wstrzyknięcie | Agent | Broker sprawdza slot, host i schemat; wykonawca dodaje sekret przez `https` |
| Maskowanie | Agent | Każda postać sekretu jest zastępowana przez `[redacted]` w tym, co wraca do modułu |

Nie ma sposobu, żeby odczytać sekret z powrotem. Nigdy nie proś użytkownika o wklejenie sekretu do parametru albo do ustawienia.

## Deklarowanie slotu

Slot to wpis w `contributes.credentials`:

```json
"credentials": [
  {
    "key": "apiToken",
    "label": "Acme API token",
    "description": "A read-only token of the Acme status API.",
    "hosts": ["status.example.com"],
    "schemes": ["bearer"],
    "setupUrl": "https://status.example.com/account/tokens"
  }
]
```

| Pole | Reguła |
| --- | --- |
| `key` | camelCase, unikalny; nazwa używana w kodzie |
| `label`, `description` | Od 1 do 48 znaków; co najwyżej 300 |
| `hosts` | Od 1 do 8 dokładnych hostów, z których każdy jest też wymieniony w polu `hosts` uprawnienia `network`. Sekret trafia do tych originów i nigdzie indziej |
| `schemes` | Niepusty podzbiór `bearer`, `basic`, `header`, `query` |
| `headerName` | Wymagane przy `header` |
| `queryName` | Wymagane przy `query` |
| `setupUrl` | Adres URL https, pod którym użytkownik tworzy sekret |
| `userOrigin` | Opcjonalne: ustawienie z adresem serwera podanym przez użytkownika; zobacz [Uprawnienia](../permissions/#adresy-serwerów-podane-przez-użytkownika) |

Wtyczka deklaruje co najwyżej 8 slotów. Do każdego slotu musi odwoływać się uprawnienie `credentials`, a każde odwołanie musi być slotem:

```json
{
  "kind": "credentials",
  "refs": ["apiToken"],
  "justification": "Sends your Acme API token to status.example.com to authenticate the status request."
}
```

## Używanie slotu

```rust
/// One authenticated GET. The plugin names the slot; the agent adds the
/// secret to the request. The secret never enters this module.
fn status_get(ctx: &mut Context<'_>, service: &str) -> Result<Response, PluginError> {
    let token = SecretHandle::slot(TOKEN_SLOT)?;
    if !ctx.has_credential(&token)? {
        return Err(PluginError::new(
            codes::CREDENTIAL_MISSING,
            "store an Acme API token in the plugin's credentials",
        ));
    }
    let request = Request::get(format!("{API}/v1/services/{service}"))
        .header("Accept", "application/json")
        .bearer(&token);
    let response = ctx.fetch(request)?;
```

### `SecretHandle::slot`

`SecretHandle::slot(key)` tworzy uchwyt z klucza slotu. Sprawdza jedynie, czy klucz jest poprawnie sformowany (camelCase, co najwyżej 32 znaki), a w przeciwnym razie zwraca `plugin-error`. Nie kontaktuje się z agentem.

### `has_credential`

`Context::has_credential(&handle)` zwraca informację, czy użytkownik zapisał sekret dla slotu: `true` albo `false`, nigdy sam sekret. Wymaga przyznanego uprawnienia do slotu.

Użyj tej metody, żeby podjąć decyzję przed wysłaniem żądania. Wtyczka, której API działa bez tokenu, może się bez niego obejść, tak jak robi to przykład Developer Tools dla publicznych repozytoriów GitHuba. Wtyczka, która potrzebuje tokenu, kończy działanie błędem ze wspólnym kodem `credential-missing`, jak wyżej.

Bez tej kontroli żądanie, które wskazuje slot bez zapisanego sekretu, kończy się błędem rodzaju `Unavailable`.

### `Request::bearer` i `Request::credential`

`Request::bearer(&handle)` dołącza slot jako token bearer. `Request::credential(&handle, scheme)` wybiera dowolny zadeklarowany schemat:

| `AuthScheme` | Jak agent wysyła sekret |
| --- | --- |
| `Bearer` | `Authorization: Bearer …` |
| `Basic` | `Authorization: Basic …` |
| `Header` | W nagłówku wskazanym przez `headerName` slotu |
| `Query` | W parametrze query wskazanym przez `queryName` slotu |

Wtyczka nie może sama ustawić nagłówka `Authorization`. Broker odrzuca ten nagłówek, podobnie jak `Cookie` i `Proxy-Authorization`, więc sloty są jedynym sposobem uwierzytelniania.

## Co wymusza broker uprawnień

Żądanie z danymi uwierzytelniającymi jest dozwolone tylko wtedy, gdy spełnione są wszystkie poniższe warunki:

- slot jest przyznany (`credentials:<key>`);
- żądanie idzie do jednego z hostów tego slotu, a ten host ma aktualnie przyznane uprawnienie `network`;
- schemat jest jednym z tych, które deklaruje slot;
- żądanie używa `https`.

Dane uwierzytelniające nigdy nie podążają za przekierowaniem do innego originu.

Cofnięcie uprawnienia sieciowego dla hosta wyłącza ten host z każdego slotu, który go wymienia. Slot, któremu nie został żaden host, w ogóle nie jest przyznany: `has_credential` i każde żądanie, które go używa, kończą się błędem `not-permitted`. Slot znów działa, gdy uprawnienie sieciowe zostanie przywrócone.

## Maskowanie sekretów

Zanim moduł zobaczy odpowiedź na żądanie, które niosło dane uwierzytelniające, agent zastępuje każdą postać sekretu (dosłowną, zakodowaną jak w formularzu i w base64) przez `[redacted]` w nagłówkach odpowiedzi, w treści i w komunikatach błędów. Na przykład API, które powtarza klucz w linku stronicowania, dociera do modułu z zamaskowanym kluczem.

Maskowanie to siatka bezpieczeństwa, a nie przyzwolenie na beztroskę. Nie kopiuj treści odpowiedzi w całości do komunikatów ani do wyjść.

## Co widzi użytkownik

Użytkownik zapisuje, zastępuje i usuwa sekret w aplikacji. Sekret ma od 1 bajta do 16 KiB i nie zawiera znaków końca wiersza. Aplikacja może pokazać, czy slot jest ustawiony, ale nigdy jego wartość. Odinstalowanie wtyczki domyślnie usuwa jej zapisane dane uwierzytelniające; zamiast tego można przy odinstalowaniu zachować ustawienia i dane uwierzytelniające na potrzeby późniejszej instalacji wtyczki o tym samym identyfikatorze.

## Testowanie

Atrapa hosta rejestruje żądanie, więc test może dowieść, że wtyczka wysłała uchwyt, a nie sekret:

```rust
    assert_eq!(fetch.url, "https://status.example.com/v1/services/api");
    // The request carries the slot's key, never a secret.
    assert_eq!(fetch.auth.as_ref().unwrap().credential, "apiToken");
    assert!(fetch.headers.iter().all(|h| !h.name.eq_ignore_ascii_case("authorization")));
```

Atrapę hosta, która odpowiada na `credentials.has`, opisuje strona [Testowanie](../testing/).

## Dokumentacja źródłowa

Sekcja „Credentials” w [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md) oraz „Plugin data” w [docs/marketplace/agent-integration.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/agent-integration.md).
