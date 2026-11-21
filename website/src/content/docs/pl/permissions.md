---
title: "Uprawnienia"
description: "Wszystkie rodzaje uprawnień w manifeście, te, które dziś faktycznie działają w ABI 1, reguły uzasadnień, zgoda użytkownika i cofanie uprawnień."
group: capabilities
order: 7
---

Wtyczka nie może zrobić niczego poza własnym modułem, jeśli nie ma na to uprawnienia. Tablica `permissions` w `manifest.json` to pełna lista tego, o co wtyczka może kiedykolwiek poprosić, użytkownik zatwierdza każdą pozycję, a broker uprawnień agenta sprawdza każde pojedyncze wywołanie funkcji hosta (capability) względem aktualnie przyznanych uprawnień. Ta strona wymienia każdy rodzaj i podaje dokładnie, które z nich mają dziś jakikolwiek skutek.

## Deklarowanie uprawnień

```json
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
]
```

Manifest ma co najwyżej 16 uprawnień i co najwyżej jedno każdego rodzaju. Jeśli potrzebujesz kilku hostów, wymień je w jednym uprawnieniu `network`. Wtyczka, która niczego nie potrzebuje, deklaruje pustą tablicę.

### Uzasadnienia

Każde uprawnienie wymaga pola `justification` o długości od 10 do 200 znaków. Okno zgody i szczegóły wtyczki pokazują je obok uprawnienia. Napisz jedno zdanie dla użytkownika, które mówi, co wtyczka robi z tym uprawnieniem. Osoby przeprowadzające przegląd w rejestrze sprawdzają, czy zgadza się ono z tym, co wtyczka robi naprawdę.

## Rodzaje uprawnień

Schemat wersji manifestu 1 definiuje dziesięć rodzajów, które może zadeklarować każda wtyczka. Cztery mają skutek w ABI 1, jeden podnosi limit, trzy mają skutek od poziomu API wtyczek podanego w tabeli, a dwa przechodzą walidację, ale na razie na nic nie pozwalają.

| Rodzaj | Pola | Status w ABI 1 |
| --- | --- | --- |
| `network` | `hosts`, `methods`, `userOrigins`, `privateNetwork` | **Działa.** Pozwala na `Context::fetch` do `https://<host>` wymienioną metodą |
| `credentials` | `refs` (od 1 do 8 kluczy slotów) | **Działa.** Pozwala dołączyć pole danych uwierzytelniających (slot) do żądania i wywołać `Context::has_credential` |
| `notifications` | `routing` (opcjonalne) | **Działa.** Pozwala na `Context::notify` |
| `openUrl` | `hosts` (od 0 do 16), `userOrigins` | **Działa.** Pozwala na `Context::open_url` dla stron `https://<host>` oraz serwera, który użytkownik wpisał i na który zezwolił |
| `storage` | `quotaKb` (od 257 do 10 240) | **Działa.** Podnosi limit pamięci trwałej z 256 KiB do `quotaKb` KiB. Sama pamięć trwała nie wymaga uprawnienia |
| `profile` | `read` (`profile`, `project`) | **API wtyczek 1.9** (DeckPilot 0.10.0). Pozwala na `Context::profile`: nazwę aktywnego profilu z `profile` oraz identyfikator, klucz i nazwę aktywnego projektu z `project`. Przed 1.9 tylko w schemacie |
| `variables` | `read` (identyfikatory zmiennych innych właścicieli) | **Tylko w schemacie.** ABI 1 nie ma funkcji hosta do odczytu zmiennych. Własne zmienne wtyczki nie wymagają uprawnienia |
| `background` | `minIntervalSeconds` (od 15 do 86 400) | **Tylko w schemacie.** ABI 1 nie ma pracy w tle ani timerów |
| `device` | `state`, `icons` (wartości logiczne) | **`state` działa** od API wtyczek 1.2: agent pyta wtyczkę, co pokazują klawisze przypisane do jej akcji ([Akcje](../actions/)); nic z urządzenia nie trafia do wtyczki. `icons` tylko w schemacie |
| `dialogs` | | **API wtyczek 1.5** (DeckPilot 0.10.0). Pozwala na `Context::dialog`: standardowe okna dialogowe rysowane przez DeckPilota, tylko w uruchomieniu rozpoczętym przez użytkownika |

Zadeklarowanie rodzaju, który na razie nic nie robi, jest dozwolone, ale i tak kosztuje użytkownika decyzję o zgodzie. Deklaruj tylko to, co twój kod faktycznie wywołuje.

Niektórych rzeczy nigdy nie da się przyznać i nie mają one w ogóle swojego rodzaju: to między innymi system plików, procesy, powłoka i schowek. Crate manifestu odrzuca takie rodzaje z jasnym komunikatem.

API wtyczek 1.9 dodaje rodzaje `tools`, `resources`, `projectContext` i `hostViews`. Mogą je deklarować wyłącznie własne wtyczki DeckPilota dostarczane wewnątrz aplikacji; każdy inny manifest, który to robi, jest odrzucany. Zobacz [Zgodność wersji](../compatibility/#api-wtyczek-19).

### Sieć

`hosts` zawiera od 1 do 16 dokładnych nazw DNS, każdą z opcjonalnym `:port`. `methods` zawiera co najmniej jedną z wartości `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`.

- Żądania idą wyłącznie przez `https`, do dokładnie tego originu, na który przyznano uprawnienie, i wymienioną metodą.
- Literały IP i `localhost` nie są poprawnymi hostami, a każdy adres, na który rozwiązuje się nazwa, musi być publiczny.
- Schemat akceptuje hosty z symbolem wieloznacznym, takie jak `*.example.com`, oraz flagę `privateNetwork`, ale **ta wersja nigdy nie przyznaje żadnego z nich**: nie są przekazywane do brokera, więc żądanie, które na nich polega, jest odrzucane.
- Każde przekierowanie jest autoryzowane od nowa jako osobne żądanie, może ich być co najwyżej 5 i nigdy z `https` na `http`.
- Niektóre nagłówki są odrzucane, między innymi `Host`, `Authorization`, `Cookie`, `Content-Length`, `Origin` oraz wszystko, co zaczyna się od `proxy-` lub `sec-`. Dane uwierzytelniające przechodzą wyłącznie przez sloty.

Limity na jedno żądanie: adres URL o rozmiarze co najwyżej 8 KiB, co najwyżej 32 nagłówki o rozmiarze co najwyżej 4 KiB, tekstowa treść żądania o rozmiarze co najwyżej 32 KiB, treść odpowiedzi o rozmiarze co najwyżej 512 KiB i 15 sekund na wszystkie przeskoki. Nagłówki odpowiedzi są filtrowane według listy dozwolonych. Status spoza zakresu od 200 do 299 to zwykły `Response`, a nie błąd.

### Dane uwierzytelniające

`refs` wymienia klucze z `contributes.credentials`. Sekret slotu trafia wyłącznie do hostów tego slotu, z których każdy musi być także hostem z przyznanym uprawnieniem `network`, i wyłącznie w zadeklarowanym schemacie. Zobacz [Dane uwierzytelniające](../credentials/).

### Powiadomienia

`Context::notify(title, body)` pokazuje powiadomienie za pośrednictwem agenta, zatytułowane nazwą wtyczki. Tytuł ma co najwyżej 64 znaki, a treść co najwyżej 300.

#### Kierowanie powiadomień

Uprawnienie może wskazać własne ustawienia wtyczki, które decydują, gdzie pojawiają się jej powiadomienia. Agent odczytuje je, tak jak zapisał je użytkownik, przy każdym powiadomieniu; sama wtyczka nigdy o tym nie decyduje.

```json
{
  "kind": "notifications",
  "routing": { "desktop": "desktop", "statusBar": "statusBar", "minimumSeverity": "minimumSeverity" },
  "justification": "Shows the notifications your workflows ask for, where you choose in the settings."
}
```

| Pole | Ustawienie, które wskazuje |
| --- | --- |
| `desktop` | Ustawienie typu `boolean`: włączone pokazuje powiadomienia także przez system operacyjny |
| `statusBar` | Ustawienie typu `boolean`: włączone wyświetla je na pasku stanu otwartego okna DeckPilot |
| `minimumSeverity` | Ustawienie typu `choice` z wartościami `info`, `success`, `warning` i `error`: mniej ważne powiadomienia nie są pokazywane |

Wymagane jest co najmniej jedno pole, a każde musi wskazywać ustawienie z `contributes.settings` odpowiedniego typu. Pominięte pole zachowuje to, co ma wtyczka bez kierowania: bez powiadomień systemowych, pasek stanu, każda ważność. Uprawnienie, jego niski poziom ryzyka, przedrostek z nazwą wtyczki i limit częstości są takie same z kierowaniem i bez niego. Agent starszy niż ta funkcja odrzuca manifest.

### Otwieranie adresów URL

`Context::open_url(url)` prosi agenta o otwarcie strony w przeglądarce użytkownika.

- Tylko adresy URL `https` na hoście, na który przyznano dokładnie takie uprawnienie, bez danych użytkownika w adresie, o rozmiarze co najwyżej 2048 bajtów. Hosty z symbolem wieloznacznym przechodzą walidację, ale nigdy nie są przyznawane.
- Tylko w uruchomieniu rozpoczętym przez użytkownika: naciśnięciem przycisku, gestem, z aplikacji albo przez workflow, który użytkownik rozpoczął w ten sposób. W uruchomieniu rozpoczętym przez harmonogram, zdarzenie cyklu życia, start agenta albo sesję pracy wywołanie jest odrzucane. `Context::run_context()` pozwala to sprawdzić zawczasu.
- `openUrl` nie daje dostępu sieciowego do hosta, a `network` nie daje prawa do otwierania stron.
- Strony serwera wpisanego przez użytkownika opisuje sekcja [Strony serwera podanego przez użytkownika](#strony-serwera-podanego-przez-użytkownika).

## Zawsze dozwolone

Włączona wtyczka może używać poniższych funkcji bez żadnego uprawnienia:

| Funkcja hosta | Ograniczenie |
| --- | --- |
| Log | 50 wierszy na sekundę |
| Własna pamięć trwała | Domyślnie 256 KiB i 1000 kluczy; 100 wywołań na sekundę |
| Własne ustawienia | |
| Kontekst uruchomienia | |
| Publikowanie i czyszczenie własnych zadeklarowanych zmiennych | 10 na sekundę, 16 KiB na wartość |

## Limity częstości

| Zakres | Limit |
| --- | --- |
| Wszystkie żądania funkcji hosta jednej wtyczki | 200 na sekundę |
| Żądania HTTP | 60 na minutę |
| Powiadomienia | 5 na minutę |
| Otwieranie adresów URL | 1 na 2 sekundy, 20 na 10 minut, 3 na wywołanie akcji |
| Wywołania funkcji hosta w jednym wywołaniu akcji | 256 |

Wywołanie ponad limit kończy się błędem rodzaju `Throttled`, który `?` zamienia na kod `throttled` z możliwością ponowienia.

## Zgoda

- **Instalacja niczego nie przyznaje.** Wtyczka jest instalowana jako wyłączona.
- **Włączenie przyznaje uprawnienia, które użytkownik zatwierdzi.** Zanim wtyczkę da się włączyć, każde zadeklarowane uprawnienie musi zostać przyznane. Wtyczka bez uprawnień jest włączana z pustą zgodą.
- Każde uprawnienie jest przyznawane pod kluczem, na przykład `network:status.example.com` dla każdego hosta, `credentials:apiToken` dla każdego slotu, `openUrl:status.example.com` dla każdego hosta, `network:user-origin:<ustawienie>` i `openUrl:user-origin:<ustawienie>` dla każdego ustawienia z adresem serwera oraz `notifications` i `storage`.
- **Aktualizacja, która dodaje albo poszerza uprawnienie, jest eskalacją.** Agent odkłada taką aktualizację jako oczekującą i prosi o zgodę na eskalowane klucze. Odmowa pozostawia starą wersję. Zobacz [Zgodność wersji](../compatibility/).

## Cofanie uprawnień

Użytkownik może w dowolnej chwili cofnąć przyznane uprawnienie i przyznać je ponownie.

- Broker odczytuje przyznane uprawnienia przy każdym wywołaniu, więc cofnięcie działa od następnego wywołania funkcji hosta, nawet w trakcie wykonywanej akcji. Wywołanie, które już trwa, nie jest przerywane.
- Odrzucone wywołanie zwraca `CapabilityError` rodzaju `NotPermitted`. W handlerze `?` zamienia go na kod `not-permitted` z komunikatem, który podaje operację, origin i uprawnienie, które trzeba zadeklarować.
- Dopóki zadeklarowane uprawnienie nie jest przyznane, akcje wtyczki są wyświetlane jako niedostępne wraz z powodem.
- Pole danych uwierzytelniających zależy od uprawnień sieciowych przyznanych dla jego hostów. Kiedy użytkownik cofnie uprawnienie dla hosta, broker wyłącza ten host z każdego slotu, który go wymienia. Slot, któremu nie został żaden host, w ogóle nie jest przyznany.
- Wyłączona albo odinstalowana wtyczka nie ma żadnych przyznanych uprawnień: każde wywołanie jest odrzucane, także te zawsze dozwolone.

Pisz handlery tak, żeby przy każdym wywołaniu spodziewały się odmowy i zwracały jej błąd.

## Adresy serwerów podane przez użytkownika

Niektóre usługi działają na hoście, który zna tylko użytkownik: na przykład samodzielnie utrzymywany serwer Git. Uprawnienie `network` może wskazać ustawienie tekstowe, w którym użytkownik podaje ten serwer. Jest to zaimplementowane na gałęzi `main`.

```json
{
  "kind": "network",
  "hosts": ["api.github.com"],
  "methods": ["GET", "POST"],
  "userOrigins": [{ "setting": "apiUrl", "pathPrefix": true, "privateNetwork": true }],
  "justification": "Reads pull requests from GitHub or your GitHub Enterprise server."
}
```

| Pole slotu | Reguła |
| --- | --- |
| `setting` | Klucz jednowierszowego ustawienia `text` w `contributes.settings` |
| `methods` | Opcjonalny podzbiór pola `methods` uprawnienia |
| `pathPrefix` | Użytkownik może podać ścieżkę; żądania są wtedy ograniczone do tej ścieżki i wszystkiego poniżej |
| `privateNetwork` | Użytkownik może podać serwer w sieci prywatnej. Adresy loopback i link-local nigdy nie są osiągalne |

Uprawnienie ma co najwyżej 4 takie sloty, a `hosts` może być puste, gdy obecne jest `userOrigins`. Pole danych uwierzytelniających można powiązać z takim slotem przez `"userOrigin": "<setting>"`.

Zgoda przebiega etapami:

1. Instalacja pokazuje slot jako osobne uprawnienie, `network:user-origin:<setting>`: wtyczka może poprosić o dostęp do serwera, który poda użytkownik.
2. Zapisanie ustawienia waliduje i normalizuje wartość: wyłącznie `https`, bez nazwy użytkownika i hasła, bez query i fragmentu, bez symbolu wieloznacznego.
3. Następnie użytkownik zezwala na dokładnie ten serwer. Serwer w sieci prywatnej to osobny, wyraźny wybór w ramach tej samej zgody.
4. Zmiana ustawienia natychmiast kończy zgodę, podobnie jak cofnięcie uprawnienia slotu albo odinstalowanie.

Broker dopasowuje każde żądanie do dozwolonego serwera dokładnie, według schematu, hosta, portu i prefiksu ścieżki, i w ten sam sposób sprawdza każde przekierowanie.

### Strony serwera podanego przez użytkownika

Pole `openUrl.hosts` zawiera stałe nazwy, więc nie obejmuje serwera znanego tylko użytkownikowi. Uprawnienie `openUrl` może wskazać ustawienia z adresem serwera, którego strony otwiera wtyczka:

```json
{
  "kind": "openUrl",
  "hosts": ["gitlab.com"],
  "userOrigins": ["apiUrl"],
  "justification": "Opens a project or a merge request on GitLab in your browser."
}
```

- Każdy wpis musi być ustawieniem (`setting`) slotu `network.userOrigins`; co najwyżej 4. Pole `hosts` może wtedy być puste.
- To osobne uprawnienie, `openUrl:user-origin:<ustawienie>`, pokazywane i przyznawane jak każde inne. Aktualizacja, która je dodaje, jest rozszerzeniem uprawnień.
- Samo w sobie niczego nie otwiera. Strony otwierają się tylko na serwerze, który użytkownik wpisał w tym ustawieniu **i na który zezwolił**, dopóki to uprawnienie jest przyznane: strony `https` o dokładnie tym schemacie, hoście i porcie. Zgoda na serwer to wyraźnie mówi.
- Dotyczy serwera, a nie przedrostka ścieżki: strony serwera zwykle nie leżą pod ścieżką jego API. Nie pozwala na żadne żądanie; żądania pozostają w granicach metod i przedrostka ścieżki, na które zezwolił użytkownik.
- Serwer w sieci prywatnej otwiera się tylko wtedy, gdy jest dokładnie tym, na który użytkownik zezwolił w sieci prywatnej.
- Obowiązują wszystkie inne zasady [otwierania adresów URL](#otwieranie-adresów-url): tylko w uruchomieniu rozpoczętym przez użytkownika i z tymi samymi limitami.
- Cofnięcie tego uprawnienia, cofnięcie zgody na serwer, zmiana ustawienia albo odinstalowanie kończy je przy następnym wywołaniu. Agent starszy niż to pole odrzuca manifest.

## Dokumentacja źródłowa

[docs/marketplace/sdk/permissions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/permissions.md) zawiera wyczerpujące reguły, w tym pełną listę odrzucanych nagłówków. Kod źródłowy brokera znajduje się w [plugins/runtime/crates/broker](https://github.com/knittingcode/deckpilot/blob/main/plugins/runtime/crates/broker/src/lib.rs).
