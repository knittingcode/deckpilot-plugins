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
| Obsługa wtyczek | Dostępna od DeckPilota 0.7.0, z wtyczkami domyślnie włączonymi. DeckPilot 0.6.0 jej nie zawiera. Agent zbudowany ze źródeł wymaga funkcji Cargo `plugins`; `--disable-plugins` lub `DECKPILOT_ENABLE_PLUGINS=0` wyłącza wtyczki |
| Publiczne wersje DeckPilota | [v0.10.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.10.0) (2026-11-21), obecne stabilne wydanie, z API wtyczek 1.9. [v0.9.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.9.0) (2026-11-20), z API wtyczek 1.4. [v0.8.1](https://github.com/knittingcode/deckpilot/releases/tag/v0.8.1) i [v0.8.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.8.0) (2026-11-19), z API wtyczek 1.3. [v0.7.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.7.0) (2026-11-18), pierwsze wydanie z obsługą wtyczek, z API wtyczek 1.2. Wcześniej v0.6.0 (2026-11-17), pierwsze stabilne wydanie, bez obsługi wtyczek, oraz wydania wstępne v0.6.0-alpha.3 i v0.6.0-alpha.4 |
| SDK dla Rusta | `deckpilot-plugin-sdk` 0.1.0, nieopublikowany w crates.io |
| SDK dla JavaScriptu | Wyłącznie deklaracje typów; nie pozwala zbudować ani uruchomić wtyczki |
| Rejestr | Działa pod adresem `https://knittingcode.github.io/deckpilot-plugins/v1/`, podpisany produkcyjnym zestawem kluczy 1. Publikuje własne wtyczki DeckPilota; nie jest jeszcze otwarty dla innych wydawców |
| Wtyczki dołączone do aplikacji | Od DeckPilota 0.10.0 Git, Docker, Workspace, Projects, GitHub, GitLab, Jira i Workflows to własne wtyczki DeckPilota dołączone do aplikacji, razem z powiadomieniami (`deckpilot.git`, `deckpilot.docker`, `deckpilot.workspace`, `deckpilot.projects`, `deckpilot.github`, `deckpilot.gitlab`, `deckpilot.jira`, `deckpilot.workflows`, `deckpilot.notifications`). Pochodzą wyłącznie z pakietu aplikacji, nigdy z rejestru |

API wtyczek 1.x (1.9 w DeckPilocie 0.10.0), ABI gościa 1 i wersja manifestu 1 są tym, co implementuje DeckPilot.

## Numery wersji

| Numer | Gdzie | Obecnie | Reguła |
| --- | --- | --- | --- |
| Wersja manifestu | `manifestVersion` | `1` | Jedyna wersja, którą agent odczytuje |
| ABI gościa | `runtime.abi`; eksport `dp_abi_version` modułu | `1` | Host ładuje wyłącznie moduły ABI 1 |
| Poziom API wtyczek | `engines.pluginApi` | DeckPilot 0.7.0 udostępnia `1.2`, 0.8.x `1.3`, 0.9.0 `1.4`, 0.10.0 `1.9` | `^MAJOR.MINOR`: ten sam numer główny, co najmniej ten numer poboczny |
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

Poziom API wtyczek, którego potrzebuje moduł, zapisywany jako `^MAJOR.MINOR`: ten sam numer główny i co najmniej ten numer poboczny. Agent z niższym poziomem nie instaluje wtyczki i zgłasza jej wersję jako niezgodną.

| DeckPilot | Udostępniany poziom API wtyczek | Wartości `pluginApi`, z którymi instalacja jest możliwa |
| --- | --- | --- |
| 0.6.0 | brak: bez obsługi wtyczek | żadne |
| 0.7.0 | 1.2 | od `^1.0` do `^1.2` |
| 0.8.0, 0.8.1 | 1.3 | od `^1.0` do `^1.3` |
| 0.9.0 | 1.4 | od `^1.0` do `^1.4` |
| 0.10.0 | 1.9 | od `^1.0` do `^1.9` |

Każdy poziom poboczny dodaje coś, z czego wtyczka może korzystać:

| Poziom | Dodaje |
| --- | --- |
| 1.1 | Czas (`Context::now`) i losowe bajty (`Context::random_bytes`) |
| 1.2 | Stan klawisza: procedura stanu akcji rysuje jej klawisz ([Akcje](../actions/)) |
| 1.3 | Dla każdej akcji: kontrolki i wejścia, na których działa (`controls`), oraz do czterech pozycji paska narzędzi (`navigation`), które otwierają widoki rysowane przez samego DeckPilota (`views`); zobacz [Format manifestu](../manifest/#kontrolki) |
| 1.4 | `context` widoku (selektory urządzenia i profilu pokazywane razem z nim) oraz `dependencies`: do 16 innych wtyczek, których wtyczka potrzebuje i które DeckPilot rozwiązuje i instaluje razem z nią przed pobraniem. `requiresDependencies` akcji wymienia opcjonalne zależności, bez których akcja nie może działać. Zależność niczego nie przyznaje |
| 1.5 | Uprawnienie `dialogs` i `Context::dialog`: standardowe okna potwierdzenia, komunikatu, formularza i wyboru rysowane przez DeckPilota, tylko w uruchomieniu rozpoczętym przez użytkownika, bez pól na sekrety |
| 1.6 | Tłumaczenia: pliki `locales/<tag>.json` w pakiecie, `locales` i `defaultLocale` w manifeście, klucze `t:` w tekstach manifestu, `Context::locale` i `Context::t` |
| 1.7 | `editorNavigation` akcji: co jej uruchomienie robi ze stroną lub układem pokazywanym przez urządzenie, na potrzeby podglądu w Device Editorze |
| 1.8 | `title` akcji (sugerowany i dynamiczny tytuł, jego położenie) oraz `contributes.visualAssets`: ikony i tła do bibliotek ikon i teł DeckPilota |
| 1.9 | Głównie dla własnych wtyczek DeckPilota dołączonych do aplikacji; zobacz [niżej](#api-wtyczek-19) |

Deklaruj `^1.0`, chyba że wtyczka nie może działać bez późniejszego dodatku. Na wywołanie, którego agent nie zna, odpowiada on, że nie jest obsługiwane, więc wtyczka, która może się bez niego obejść, zostaje przy `^1.0` i działa w każdym DeckPilocie z obsługą wtyczek. Wtyczka deklarująca wyższy poziom podnosi go także w kodzie, za pomocą `Plugin::requires_api`; `testing::manifest_mismatches` porównuje oba.

Manifest, który używa `controls`, `navigation` lub `views`, musi deklarować `"pluginApi": "^1.3"`; walidator manifestu odrzuca niższy poziom, a agenty poniżej 1.3 i tak odrzucają taki manifest. Ponieważ DeckPilot 0.8.0 jest pierwszym wydaniem udostępniającym 1.3, taka wtyczka powinna też deklarować `"deckpilot": ">=0.8.0"` (na przykład `">=0.8.0 <1.0.0"`), aby DeckPilot 0.7.0 jej nie oferował. Wtyczki deklarujące od `^1.0` do `^1.2` nadal działają w DeckPilocie 0.8.0.

To samo dotyczy późniejszych poziomów: manifest, który korzysta z dodatku z poziomów od 1.4 do 1.8, musi deklarować ten poziom i powinien podać pierwsze wydanie, które go udostępnia, jako dolną granicę `engines.deckpilot`: `">=0.9.0"` dla 1.4, `">=0.10.0"` dla poziomów od 1.5 do 1.8. Każda wtyczka deklarująca niższy poziom nadal działa w DeckPilocie 0.10.0.

### API wtyczek 1.9

API wtyczek 1.9 to podstawa, na której DeckPilot 0.10.0 przeniósł Gita, Dockera, Workspace, Projects i Workflows ze swojego rdzenia do własnych wtyczek dołączonych do aplikacji. Jego dodatki to uprawnienia `tools` (pośredniczone uruchomienia `git` i `docker`), `resources`, `projectContext` i `hostViews`, `openUrl` z `fromParams`, parametry `resource`, `requiresPermissions` akcji oraz zmienne, których wartość DeckPilot wylicza z aktywnego projektu. Wszystkie są akceptowane **wyłącznie** od własnych wtyczek DeckPilota dostarczanych wewnątrz aplikacji, których każdy plik zgadza się z odciskami wkompilowanymi w agenta. Manifest wtyczki zewnętrznej, pakiet z rejestru (także pakiet `deckpilot.*`) oraz pakiet instalowany ręcznie lub deweloperski, który deklaruje którykolwiek z nich, są odrzucane (`first-party-only`); narzędzia rejestru również je odrzucają. Ogólne uruchamianie procesów, powłoka, system plików i kod interfejsu wtyczki pozostają niemożliwe do przyznania dla każdego wydawcy.

Jedynym dodatkiem 1.9 dostępnym dla każdego wydawcy jest uprawnienie `profile` (`Context::profile`: nazwa aktywnego profilu oraz, z `project`, aktywny projekt); zobacz [Uprawnienia](../permissions/). Wtyczka zewnętrzna, która z niego nie korzysta, nie ma powodu deklarować `^1.9`.

Pełne reguły są w [dokumentacji uprawnień](https://github.com/knittingcode/deckpilot/blob/v0.10.0/docs/marketplace/sdk/permissions.md#plugin-api-19-first-party-bundled-plugins-only) DeckPilota.

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
