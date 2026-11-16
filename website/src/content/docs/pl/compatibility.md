---
title: "Zgodność wersji"
description: "Numery wersji wtyczki: engines.deckpilot, pluginApi, sdk, ABI gościa, manifestVersion, platformy, semver wtyczki oraz działanie aktualizacji."
group: reference
order: 16
---

Wtyczka ma kilka numerów wersji, a każdy odpowiada na inne pytanie: na których wersjach DeckPilota działa, jakiego API wtyczek potrzebuje, jakim interfejsem binarnym posługuje się jej moduł. Ta strona je wymienia, podaje ich obecne wartości i opisuje, jak DeckPilot korzysta z nich przy instalowaniu i aktualizowaniu wtyczki.

## Obecny stan

| Fakt | Obecnie |
| --- | --- |
| Obsługa wtyczek | Eksperymentalna. Wyłączona w każdej wydanej wersji DeckPilota: agent wymaga funkcji Cargo `plugins` i flagi `--enable-plugins` |
| Publiczne wersje DeckPilota | Jedno wydanie wstępne, v0.6.0-alpha.3. Nie ma wydania stabilnego |
| SDK dla Rusta | `deckpilot-plugin-sdk` 0.1.0, nieopublikowany w crates.io |
| SDK dla JavaScriptu | Wyłącznie deklaracje typów; nie pozwala zbudować ani uruchomić wtyczki |
| Rejestr | Brak kluczy produkcyjnych, brak opublikowanych wtyczek, nie jest otwarty na publikacje |

API wtyczek 1.0, ABI gościa 1 i wersja manifestu 1 są tym, co implementuje gałąź `main`. Mogą się jeszcze zmienić, zanim wtyczki zostaną udostępnione.

## Numery wersji

| Numer | Gdzie | Obecnie | Reguła |
| --- | --- | --- | --- |
| Wersja manifestu | `manifestVersion` | `1` | Jedyna wersja, którą agent odczytuje |
| ABI gościa | `runtime.abi`; eksport `dp_abi_version` modułu | `1` | Host ładuje wyłącznie moduły ABI 1 |
| Poziom API wtyczek | `engines.pluginApi` | `1.0` | `^MAJOR.MINOR`: ten sam numer główny, co najmniej ten numer poboczny |
| Wersja DeckPilota | `engines.deckpilot` | `>=0.5.0 <1.0.0` w przykładach | Zakres dopasowywany do wersji agenta |
| Wersja SDK | `engines.sdk` | `0.1.0` | Informacyjna |
| Wersja wtyczki | `version` | twoja | Wersjonowanie semantyczne z ograniczoną postacią wydania wstępnego |

## Engines

```json
"engines": { "deckpilot": ">=0.5.0 <1.0.0", "pluginApi": "^1.0", "sdk": "0.1.0" }
```

### `engines.deckpilot`

Wersje DeckPilota, na których wtyczka działa. To jeden komparator albo dolna i górna granica rozdzielone jedną spacją. Komparator to `>=`, `>`, `<=`, `<` lub `=` (albo brak, co oznacza `=`), po którym następuje `MAJOR.MINOR.PATCH` bez zer wiodących i bez części wydania wstępnego. Zakres musi być spełnialny i ma najwyżej 64 znaki.

Dopasowanie stosuje kolejność wersjonowania semantycznego i obejmuje wydania wstępne DeckPilota:

- `0.6.0-alpha.3` spełnia `>=0.5.0 <1.0.0`.
- `0.8.0-beta.1` nie spełnia `>=0.8.0`, ponieważ wydanie wstępne poprzedza swoje wydanie.

Klient rejestru sprawdza zakres przed instalacją, a agent sprawdza go względem własnej wersji. Niezgodna wersja nie jest oferowana do instalacji.

### `engines.pluginApi`

Poziom API wtyczek, którego potrzebuje moduł, zapisywany jako `^MAJOR.MINOR`. Agent udostępnia poziom 1.0, więc `^1.0` to jedyna wartość, z którą instalacja jest dziś możliwa.

### `engines.sdk`

Wersja SDK, za pomocą której zbudowano wtyczkę. Ma charakter informacyjny. Odczytuje ją jedynie pomocnicza funkcja do testów jednostkowych `testing::manifest_mismatches`, która porównuje ją z SDK, względem którego kompilowany jest test.

## ABI gościa

`"runtime": { "kind": "wasm", "abi": 1 }` deklaruje interfejs binarny. `wasm` to jedyny rodzaj, a `1` to jedyne ABI. Manifest bez `runtime` opisuje starszy projekt oparty na JavaScripcie, a jego pakiety są odrzucane.

ABI 1 rozrastało się w sposób zgodny, a reguły tego rozwoju chronią istniejące moduły:

- Dodawane elementy to opcjonalne eksporty i nowe operacje funkcji hosta (capability). Moduł, który z nich nie korzysta, pozostaje bez zmian.
- Operacja, której starszy host lub agent nie zna, jest odrzucana z błędem i nigdy nie jest błędnie interpretowana. W SDK jest to rodzaj błędu `Unsupported`.
- Pole dodane do tego, co wysyła host, trafia tylko do modułów, które przez opcjonalny eksport `dp_features` deklarują, że je odczytują. Robi to każdy moduł zbudowany za pomocą bieżącego SDK.

Dla kodu wtyczki oznacza to, że wywołanie takie jak `Context::run_context` lub `Context::notify_with` może się nie powieść na agencie starszym niż dana operacja. Potraktuj to jako „nieznane” i zastosuj rozwiązanie zastępcze.

## Crate SDK

`deckpilot-plugin-sdk` ma własną wersję semantyczną, niezależną od wersji DeckPilota. Dopóki ma numer 0.x, wersja poboczna może złamać zgodność API Rusta. Crate nie jest opublikowany, więc przypnij commit DeckPilota, względem którego budujesz; zobacz [Pierwsze kroki](../getting-started/).

## Platformy

`platforms` to opcjonalna lista wartości `macos`, `windows` i `linux`. Pominięcie oznacza wszystkie trzy. Wtyczka nie jest oferowana na platformie, której nie wymienia.

Agent uruchamia wtyczki tylko tam, gdzie host ma pełną piaskownicę systemu operacyjnego, a ta istnieje dla macOS, Linuksa i Windows. Działanie w systemie Windows zostało zweryfikowane wyłącznie na runnerze CI.

## Wersje wtyczki

`version` ma postać `X.Y.Z` z opcjonalnym wydaniem wstępnym o dokładnie takiej postaci: `-alpha.N`, `-beta.N` lub `-rc.N`, gdzie N wynosi co najmniej 1. Metadane buildu i zera wiodące są niedozwolone, a cały ciąg ma najwyżej 32 znaki. Musi być równy wersji przekazanej do `Plugin::new`.

W rejestrze wersje są wyłącznie dopisywane:

- Nowa wersja musi być wyższa od każdej zapisanej.
- Zapisana wersja jest niezmienna. Nie można jej edytować ani usunąć.
- Wersję może wycofać właściciel repozytorium przez odwołanie; zobacz [Podpisywanie i zaufanie](../signing/#odwołanie).

DeckPilot nie narzuca, co dla twojej wtyczki oznacza zmiana główna, poboczna lub poprawkowa. Rozsądną konwencją jest traktowanie identyfikatorów akcji, kluczy parametrów, kluczy danych wyjściowych i identyfikatorów zmiennych jako publicznego interfejsu, ponieważ odwołują się do nich przyciski i workflow użytkowników. Aby wycofać akcję, oznacz ją jako `deprecated` i wskaż jej następcę w `replacedBy`, zamiast od razu ją usuwać.

## Aktualizacje i eskalacja uprawnień

Aktualizacje porównują wersje zgodnie z wersjonowaniem semantycznym. Gdy aktualizacja jest dostępna, dalszy przebieg zależy od jej uprawnień:

- **Te same uprawnienia lub mniej.** Aktualizację można zastosować.
- **Dodane lub rozszerzone uprawnienia.** To eskalacja. Agent przygotowuje aktualizację i pokazuje eskalowane uprawnienia w oknie zgody. Zatwierdzenie stosuje aktualizację; odmowa pozostawia starą wersję, bez nowego uprawnienia.

Przykłady eskalacji: nowy rodzaj uprawnienia, kolejny host sieciowy, kolejna metoda HTTP, pole danych uwierzytelniających (slot) powiązane z nowym hostem, większy limit pamięci trwałej.

Co aktualizacja zachowuje, a co usuwa:

| Stan | Po aktualizacji |
| --- | --- |
| Pamięć trwała | Zachowana |
| Ustawienia i dane uwierzytelniające | Zachowane. Zapisane ustawienie, które nie pasuje już do swojego pola, wraca do wartości domyślnej |
| Opublikowane zmienne | Usunięte, dopóki akcja nie opublikuje ich ponownie |
| Przyznane uprawnienia | Zachowane; eskalowane klucze wymagają nowej zgody |

Użytkownik może też przywrócić poprzednią wersję. Zaplanuj przechowywane dane tak, aby starsza wersja twojej wtyczki nie zawodziła na wartościach zapisanych przez nowszą.

## Narzędzia rejestru i przypięcie DeckPilota

Repozytorium rejestru buduje swoje narzędzia i kopiuje swoje schematy z jednego przypiętego commita DeckPilota. Funkcje manifestu dodane do DeckPilota po tym commicie nie przechodzą walidacji w rejestrze, dopóki przypięcie nie zostanie przesunięte.

## Dokumentacja źródłowa

Sekcja Versions w [docs/marketplace/sdk/errors-and-versions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/errors-and-versions.md) oraz tabela zgodności w [docs/marketplace/sdk/manifest.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md).
