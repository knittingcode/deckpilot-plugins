---
title: "Architektura"
description: "Działanie wtyczki DeckPilota: jeden moduł WebAssembly w procesie hosta w piaskownicy, broker uprawnień dla każdego wywołania i podpisany statyczny rejestr."
group: start
order: 2
---

Wtyczka to kod niezaufany. DeckPilot nigdy nie uruchamia jej wewnątrz aplikacji ani agenta działającego w tle: wtyczka działa w osobnym procesie zamkniętym w piaskownicy, a wszystko, czego chce od świata zewnętrznego, jest żądaniem, które agent sprawdza względem tego, co zatwierdził użytkownik. Ta strona opisuje poszczególne elementy i to, jak do siebie pasują.

> **Status:** Wszystko tutaj opisuje gałąź `main` DeckPilota. DeckPilot 0.7.0 jest pierwszym wydaniem z obsługą wtyczek; bieżące wydanie 0.10.0 udostępnia API wtyczek 1.9. Podpisany rejestr działa. Zobacz [Zgodność wersji](../compatibility/).

## Elementy

```text
 configuration app
        |  IPC
        v
 +---------------------- agent ------------------------+
 |  plugin store      registry client     package      |
 |  (encrypted        (verifies           verification |
 |   database)         signatures)                     |
 |                                                     |
 |  permission broker  <----  executor                 |
 |  (grants, limits)          (HTTP, storage, notify)  |
 +---------|-------------------------------------------+
           |  one supervisor per running plugin
           v
 +---- deckpilot-plugin-host (OS sandbox) ----+
 |  wasmtime, no WASI                          |
 |  plugin.wasm  --host_call-->  capability    |
 +---------------------------------------------+
```

| Element | Rola |
| --- | --- |
| `plugin.wasm` | Twój kod: jeden moduł WebAssembly implementujący ABI gościa 1 |
| `manifest.json` | Jedyna rzecz, którą agent czyta, żeby dowiedzieć się, czym jest wtyczka, o co prosi i co oferuje. Agent nigdy nie uruchamia kodu wtyczki, żeby poznać metadane |
| Host wtyczki | Osobny proces `deckpilot-plugin-host`, ograniczony piaskownicą systemu operacyjnego, który uruchamia moduł za pomocą wasmtime |
| Nadzorca (supervisor) | Uruchamia jeden proces hosta na każdą działającą wtyczkę, wymusza limity i restartuje go po awarii |
| Broker uprawnień | Rozstrzyga każde żądanie funkcji hosta (capability) na podstawie aktualnych uprawnień przyznanych przez użytkownika |
| Wykonawca (executor) | Wykonuje dozwolone żądanie w agencie: wywołanie HTTP, zapis do pamięci trwałej, powiadomienie |
| Rejestr | Podpisane statyczne metadane oraz pakiety wtyczek, publikowane z osobnego repozytorium |

## Moduł

Wykonywalnym artefaktem jest dokładnie jeden moduł WebAssembly, dostarczany jako `plugin.wasm` i zadeklarowany w manifeście jako `"runtime": { "kind": "wasm", "abi": 1 }`. Kontrola pakietu waliduje go statycznie, bez kompilowania i bez uruchamiania:

- jest to moduł core WebAssembly 2.0 (bez komponentów, wątków, pamięci 64-bitowej, wielu pamięci, GC i wyjątków);
- jego jedynym importem jest `deckpilot_v1.host_call`;
- eksportuje `memory`, `dp_abi_version`, `dp_alloc` i `dp_invoke`; opcjonalne eksporty `dp_describe`, `dp_init`, `dp_shutdown` i `dp_features` muszą mieć dokładnie określone typy;
- ma co najwyżej 4 MiB.

Nie ma WASI. Moduł nie ma własnego systemu plików, sieci, zegara, źródła losowości, środowiska ani procesu. SDK dla Rusta generuje wszystkie eksporty jednym makrem, więc kod wtyczki nigdy nie styka się bezpośrednio z ABI; zobacz [Przegląd SDK](../sdk/).

Kontrakt jest celowo niezależny od języka, ale dziś SDK dla Rusta to jedyny sposób zbudowania takiego modułu. Wtyczek w JavaScripcie nie da się zbudować ani uruchomić.

## Proces hosta w piaskownicy

Agent uruchamia proces hosta dla wtyczki na żądanie, przy jej pierwszym wywołaniu. Przed każdym uruchomieniem weryfikuje zainstalowane pliki względem ich rekordu integralności i ponownie waliduje `plugin.wasm`.

Proces hosta zamyka się w piaskownicy, zanim przeczyta choćby bajt kodu wtyczki. Agent akceptuje tylko taki proces hosta, który zgłasza pełne wymuszenie piaskownicy:

| Platforma | Mechanizm |
| --- | --- |
| macOS | Seatbelt, z profilem, który domyślnie zabrania wszystkiego |
| Linux | seccomp razem z Landlock |
| Windows | AppContainer bez capabilities oraz Job Object |

Proces hosta, który zgłasza tylko częściowe wymuszenie, jest zatrzymywany, zanim otrzyma moduł. Działanie w systemie Windows zostało zweryfikowane wyłącznie na runnerze CI, nie na desktopowych edycjach Windows.

Wewnątrz procesu hosta każde wywołanie działa w ramach limitów: limitu czasu rzeczywistego, terminu CPU między dwoma wywołaniami hosta, budżetu paliwa (fuel), 64 MiB pamięci liniowej i co najwyżej 256 wywołań funkcji hosta. Dokładne wartości podaje strona [Cykl życia](../lifecycle/).

## Broker uprawnień

Kiedy kod wtyczki wywołuje na przykład `Context::fetch`, SDK wysyła przez `host_call` jedno żądanie funkcji hosta. Proces hosta przekazuje je agentowi. Żądanie nigdy nie zawiera identyfikatora wtyczki: agent wie, proces hosta której wtyczki pyta.

Następnie broker sprawdza żądanie względem aktualnych uprawnień przyznanych wtyczce. Uprawnienia są domyślnie odmawiane, manifest wymienia wszystko, o co wtyczka może kiedykolwiek poprosić, a użytkownik zatwierdza każdą pozycję. Ponieważ broker odczytuje przyznane uprawnienia przy każdym wywołaniu, cofnięcie uprawnienia działa od następnego wywołania, nawet w trakcie wykonywanej akcji.

Sekrety idą tą samą drogą w przeciwnym kierunku. Wtyczka wskazuje pole danych uwierzytelniających (slot), a agent wstrzykuje zapisany sekret do wychodzącego żądania. Sekret nigdy nie trafia do pamięci modułu. Zobacz [Uprawnienia](../permissions/) i [Dane uwierzytelniające](../credentials/).

## Rejestr

Wtyczki są dystrybuowane niezależnie od aplikacji, z repozytorium [knittingcode/deckpilot-plugins](https://github.com/knittingcode/deckpilot-plugins):

- **Metadane rejestru** to zbiór statycznych, podpisanych dokumentów JSON serwowanych przez GitHub Pages: zestawy kluczy podpisane kluczem głównym (root), podpisany indeks wszystkich wtyczek, adresowane treścią szczegóły wtyczek oraz znacznik czasu (timestamp).
- **Pakiety wtyczek** (`<id>-<version>.dpplugin`) to zasoby (assets) GitHub Releases tego repozytorium, po jednym wydaniu na każdą wersję wtyczki.

Autorzy wtyczek zgłaszają je przez pull request. Rejestr buduje każdy pakiet od nowa z otagowanych źródeł autora, a właściciel repozytorium przeprowadza przegląd i publikuje. Repozytorium aplikacji nigdy nie publikuje wtyczki, a rejestr nigdy nie publikuje wydania aplikacji.

## Łańcuch zaufania

DeckPilot ufa rejestrowi wyłącznie poprzez podpisy, które sam weryfikuje. Hosting i TLS nie są częścią zaufania.

1. Publiczne klucze główne (root) są wkompilowane w aplikację.
2. Klucze główne podpisują zestawy kluczy, które wskazują klucz indeksu i klucz znacznika czasu.
3. Klucz znacznika czasu podpisuje krótko ważny znacznik czasu, który przypina jeden indeks przez jego hash.
4. Klucz indeksu podpisuje indeks, który przypina dokument szczegółów każdej wtyczki przez jego hash.
5. Dokument szczegółów przypina każdą wersję pakietu przez sha256 i rozmiar.
6. Pobrany pakiet jest instalowany tylko wtedy, gdy jego bajty zgadzają się z tymi przypięciami i przechodzi on kontrole pakietu.

Przejęty serwer hostujący rejestr może zatem wstrzymywać aktualizacje, co klienty wykrywają jako nieaktualny rejestr, ale nie może sprawić, że klient zaakceptuje pakiet. Autorzy wtyczek nie mają żadnych kluczy i niczego nie podpisują; zobacz [Podpisywanie i zaufanie](../signing/).

## Więcej informacji

- [Kontrakt artefaktu](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/artifact-contract.md): reguły dotyczące modułu i pakietu.
- [Wtyczki w agencie](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/agent-integration.md): szczegółowy opis backendu.
- [Runtime wtyczek](https://github.com/knittingcode/deckpilot/blob/main/plugins/runtime/README.md): ABI, proces hosta, piaskownica i nadzorca.
- [Architektura rejestru](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/architecture.md) i [model zaufania](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/trust-model.md).
