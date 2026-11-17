---
title: "Podpisywanie i zaufanie"
description: "Autorzy wtyczek niczego nie podpisują: rejestr podpisują role minisign, a pakiety są przypięte w podpisanym indeksie przez sha256 i rozmiar."
group: ship
order: 14
---

Autorzy wtyczek niczego nie podpisują. Nie ma klucza autora, certyfikatu do kupienia ani pliku podpisu w pakiecie. Podpisywany jest rejestr: niewielki zbiór statycznych dokumentów, które przypinają każdy opublikowany pakiet przez jego sha256 i rozmiar. Ta strona wyjaśnia, kto co podpisuje i co klient DeckPilota weryfikuje, zanim zainstaluje wtyczkę.

> **Status:** Rejestr produkcyjny pod adresem `https://knittingcode.github.io/deckpilot-plugins/v1/` jest podpisany zestawem kluczy 1: dwoma kluczami głównymi przechowywanymi offline, z progiem 1 z 2, oraz osobnymi kluczami indeksu i znacznika czasu. Znacznik czasu jest codziennie podpisywany na nowo. Publiczne klucze główne znajdują się w pliku [`keys/roots.json`](https://github.com/knittingcode/deckpilot-plugins/blob/main/keys/roots.json) repozytorium rejestru, z identyfikatorami kluczy `131875735F6B6F06` i `9D7D888C7D407086`. Następne wydanie DeckPilota będzie miało je wbudowane; DeckPilot 0.6.0 nie obsługuje wtyczek.

## Co dostarcza autor

Autor dostarcza kod źródłowy w otagowanym commicie w publicznym repozytorium GitHub oraz pliki zgłoszenia opisane na stronie [Zgłaszanie wtyczki](../submission/). Rejestr buduje pakiet z tego kodu źródłowego. Tożsamością autora jest konto GitHub będące właścicielem rekordu wydawcy, powiązane z jego liczbowym identyfikatorem użytkownika GitHub.

Sam pakiet nie zawiera żadnego podpisu. Jego autentyczność wynika w całości z podpisanych dokumentów rejestru, które podają jego skrót.

## Podpisane dokumenty

Metadane rejestru są udostępniane jako pliki statyczne. Każdy podpis jest podpisem minisign (Ed25519).

| Dokument | Podpisany przez | Zawiera |
| --- | --- | --- |
| `keys/<n>.json` | klucze główne (root) | Zestaw kluczy: klucze publiczne root, indeksu i znacznika czasu (timestamp) |
| `index/<seq>.json` | klucz indeksu | Każdą wtyczkę, wydawców, odwołania oraz skrót dokumentu szczegółów każdej wtyczki |
| `plugins/<id>/<sha256>.json` | sam nie jest podpisany; przypięty przez skrót w indeksie | Szczegóły wtyczki: jej wersje, każda z sha256 pakietu, rozmiarem i uprawnieniami |
| `timestamp.json` | klucz znacznika czasu | Numer sekwencyjny, długość i sha256 bieżącego indeksu, z terminem ważności |

Zaufany komentarz (trusted comment) każdego podpisu podaje jego rolę, a dla indeksu i znacznika czasu także numer sekwencyjny i zestaw kluczy. Komentarz musi się zgadzać dokładnie, więc podpis dla jednej roli lub jednego numeru sekwencyjnego nigdy nie zostanie przyjęty jako inny. Podpis aktualizatora aplikacji DeckPilot nigdy nie zostanie przyjęty jako podpis rejestru, ani odwrotnie.

## Trzy role

| Rola | Podpisuje | Gdzie ma się znajdować klucz prywatny | Co oznaczałoby jego przejęcie |
| --- | --- | --- | --- |
| root | Zestawy kluczy | Wyłącznie offline, nigdy na GitHubie | Pełne zaufanie do rejestru, dopóki wydanie DeckPilota nie dostarczy nowych kluczy głównych |
| index | Indeks | Sekret chronionego środowiska, które uruchamia się tylko dla tagów wydań, a te mogą tworzyć wyłącznie administratorzy | Złośliwe wpisy wtyczek do czasu rotacji klucza |
| timestamp | Znacznik czasu | Sekret środowiska publikowania i środowiska codziennego odświeżania | Wyłącznie zamrożenie lub opóźnienie: nie może wskazać indeksu, którego nie podpisał klucz indeksu |

Zestaw kluczy, w którym dwie role współdzielą klucz, jest odrzucany, więc działający bez nadzoru klucz znacznika czasu nigdy nie może wystąpić w roli klucza indeksu ani klucza głównego.

## Co weryfikuje klient

Zanim DeckPilot cokolwiek zainstaluje, jego klient rejestru sprawdza po kolei:

1. **Zestawy kluczy.** Począwszy od publicznych kluczy głównych wbudowanych w aplikację, każdy zestaw kluczy wymaga progu podpisów poprzedniej roli root, a także nowej, jeśli klucze główne się zmieniają.
2. **Znacznik czasu.** Podpisany bieżącym kluczem znacznika czasu, wskazujący bieżący zestaw kluczy, nieprzeterminowany, ważny najwyżej 14 dni i z numerem sekwencyjnym, który nigdy nie maleje.
3. **Indeks.** Jego długość i sha256 muszą być równe temu, co przypina znacznik czasu, zanim w ogóle zostanie sparsowany; musi być podpisany kluczem indeksu oraz mieć ten sam numer sekwencyjny i zestaw kluczy.
4. **Szczegóły.** Długość i sha256 dokumentu muszą być równe temu, co przypina indeks; wersje ściśle rosną; adresy URL pakietów leżą w obrębie plików do pobrania z wydań repozytorium rejestru.
5. **Pakiet.** Jego sha256 i rozmiar muszą być równe wartościom przypiętym w szczegółach, zanim zostanie rozpakowany. Potem wykonywane są kontrole pakietu: skrót manifestu, uprawnienia bajt po bajcie względem tego, co pokazało okno zgody, oraz statyczna kontrola `plugin.wasm`.

Hosting i TLS nie są częścią tego łańcucha. Pliki mogłyby być udostępniane skądkolwiek; liczą się tylko podpisy i skróty.

## Wynikające z tego zabezpieczenia

| Zagrożenie | Zabezpieczenie |
| --- | --- |
| Zmodyfikowany pakiet lub plik metadanych | Skrót lub podpis się nie zgadza; odrzucenie |
| Cofnięcie do starszego, poprawnie podpisanego stanu rejestru | Numer sekwencyjny nigdy nie maleje; klient przechowuje najwyższy, jaki widział |
| Dwa różne indeksy pod tym samym numerem sekwencyjnym | Przy równym numerze sekwencyjnym znacznik czasu musi przypinać ten sam indeks |
| Zamrożenie klientów na starym stanie | Ograniczone terminem ważności znacznika czasu. Jest on podpisywany na 7 dni i ma być podpisywany ponownie codziennie; klienci odrzucają znacznik ważny dłużej niż 14 dni |
| Cofnięcie zegara komputera | Klienci przechowują dolną granicę zegara z najnowszego zweryfikowanego znacznika czasu |

Gdy zapisany znacznik czasu wygaśnie, klient pokazuje rejestr jako nieaktualny, odmawia instalacji i aktualizacji oraz nadal uruchamia zainstalowane wtyczki, egzekwując odwołania.

## Odwołanie

Właściciel repozytorium może wycofać opublikowaną wersję w podpisanym indeksie:

| Działanie | Zainstalowana kopia | Nowe instalacje i aktualizacje |
| --- | --- | --- |
| `yank` | Działa dalej, z powiadomieniem | Nie są oferowane |
| `disable` | Zatrzymana i wyłączona, z podaniem powodu | Odrzucane |

Odwołania się kumulują: nowy indeks musi zachować każde opublikowane odwołanie w co najmniej tak samo silnej postaci, a klienci odrzucają indeks, który jakieś pomija. Odwołania docierają do klienta przy jego następnym udanym odświeżeniu i od tej chwili są egzekwowane offline.

## Po instalacji

Agent zapisuje integralność zainstalowanych plików. Przed każdym uruchomieniem procesu hosta wtyczki weryfikuje pliki względem tego zapisu i ponownie waliduje moduł.

## Lokalne klucze do testów

`deckpilot-registry keys generate` tworzy pary kluczy do użytku lokalnego, dzięki czemu możesz zbudować prywatny rejestr na własnej maszynie i przetestować cały przebieg instalacji; zobacz [Testowanie](../testing/#z-prawdziwym-agentem). Agent ufa takiemu rejestrowi tylko wtedy, gdy został uruchomiony z opcją `--plugin-registry-roots` wskazującą plik `roots.json` tego rejestru. Te klucze nie mają żadnego związku z publicznym rejestrem.

## Co pozostało przed otwarciem rejestru dla innych wydawców

Klucze produkcyjne istnieją, a procesy publikowania podpisują nimi działający rejestr. DeckPilot będzie z niego korzystać od następnego wydania, które ma wbudowane klucze główne. To, czego według repozytorium rejestru brakuje przed otwarciem go dla wtyczek innych wydawców, opisuje lista kontrolna właściciela repozytorium, [docs/production-readiness.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/production-readiness.md), w sekcji „Before opening the registry to the public”.

## Dokumentacja źródłowa

[docs/trust-model.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/trust-model.md) w repozytorium rejestru oraz [README](https://github.com/knittingcode/deckpilot/blob/main/crates/deckpilot-plugin-registry/README.md) klienta i [PUBLISHING.md](https://github.com/knittingcode/deckpilot/blob/main/crates/deckpilot-plugin-registry/PUBLISHING.md) w repozytorium DeckPilota.
