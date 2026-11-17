# Pomodoro (`deckpilot.pomodoro`)

Minutnik skupienia z krótkimi i długimi przerwami: powiadomienie na koniec
fazy, opcjonalne automatyczne uruchamianie następnej fazy i statystyki
ukończonych sesji skupienia.

English: [README.md](README.md).

> **Najpierw przeczytaj.** Wtyczka DeckPilot nie ma własnego odliczania
> i działa tylko wtedy, gdy działa któraś z jej akcji. Sama z siebie
> niczego więc nie odlicza: **dodajesz jeden przepływ pracy, który co
> minutę uruchamia jej akcję Takt** ([Konfiguracja](#konfiguracja)).
> Minutnik liczy **pełne minuty, nie sekundy**: przesuwa się przy każdym
> takcie ([Czego nie robi](#czego-nie-robi)).
>
> **Odliczanie na klawiszu** (od wersji 0.2.0, DeckPilot z API wtyczek
> 1.2): klawisz przypisany do akcji **Stan** pokazuje pozostałe minuty,
> fazę i jej postęp ([Na klawiszu](#na-klawiszu)).

## Konfiguracja

Raz w każdym profilu, w którym chcesz używać minutnika. Wyzwalacze
przepływów pracy działają tylko dla aktywnego profilu i gdy konfiguracja
DeckPilot jest odblokowana.

1. Zainstaluj i włącz wtyczkę; zezwól na jej jedyne uprawnienie,
   powiadomienia.
2. Utwórz przepływ pracy, na przykład „Takt Pomodoro”.
3. W panelu przepływu pracy (nic nie jest zaznaczone) dodaj **wejście**:
   klucz `firedAt`, typ tekst, wymagane.
4. Dodaj **wyzwalacz**: harmonogram, co **1** minutę. Dla wejścia
   `firedAt` wybierz pole zdarzenia **Date and time** (data i godzina).
5. Za węzłem startowym dodaj jeden węzeł akcji: **Pomodoro › Tick**. Jego
   parametr **Date and time** zmapuj na wyjście `firedAt` węzła startowego
   (czyli na wejście przepływu pracy).
6. Zapisz. Same takty niczego nie uruchamiają: minutnik czeka na Start.
7. Przypisz do klawisza **Start, pause or resume** (`toggle`). Jeśli
   chcesz, dodaj na innych klawiszach **Skip phase**, **Reset** i
   **Status**.

Nazwy w aplikacji są po angielsku, bo API wtyczek 1.0 nie ma tłumaczeń
metadanych; polskie odpowiedniki są w [`locales/pl.json`](locales/pl.json).

W zapisanym profilu istotne części wyglądają tak (węzeł akcji wskazuje
akcję typu `deckpilot.pomodoro.tick`):

```json
{
  "inputs": [{ "key": "firedAt", "label": "Date and time", "kind": "text", "required": true }],
  "triggers": [
    { "type": "schedule", "schedule": { "every": "interval", "minutes": 1 },
      "inputs": { "firedAt": { "type": "event", "field": "firedAt" } } }
  ],
  "graph": { "nodes": [
    { "id": "start", "type": "trigger" },
    { "id": "tick", "type": "action", "action": "<akcja Tick>",
      "mappings": { "firedAt": { "type": "template", "template": "{{ steps.start.output.firedAt }}" } } }
  ] }
}
```

Data i godzina wyzwalacza (`RRRR-MM-DD GG:MM`, czas lokalny komputera) to
jedyny zegar wtyczki. Harmonogram rzadszy niż co minutę też działa,
z fazami kończącymi się najwyżej o tyle później, pod warunkiem że jest
**krótszy niż ustawienie „Away after”** (domyślnie 5 minut): dłuższa pauza
między dwoma taktami wstrzymuje minutnik.

## Cykl

Sesja skupienia 1, krótka przerwa, sesja skupienia 2, krótka przerwa, …
a po ostatniej sesji skupienia w cyklu (domyślnie czwartej) długa przerwa;
potem wszystko zaczyna się od sesji 1. Przy domyślnych ustawieniach
przerwa uruchamia się sama, gdy kończy się sesja skupienia, a następna
sesja skupienia czeka na Start.

## Akcje

| Akcja | Co robi |
| --- | --- |
| `tick` (Takt) | Odejmuje od biegnącego odliczania minuty, które minęły od ostatniego taktu; gdy faza się kończy, pokazuje powiadomienie i przechodzi do następnej. Parametr `firedAt`: data i godzina wyzwalacza. Dla przepływu pracy, nie dla klawisza |
| `start` (Start) | Uruchamia odliczanie bieżącej fazy albo wznawia wstrzymane |
| `pause` (Wstrzymaj) | Zatrzymuje odliczanie w miejscu |
| `resume` (Wznów) | Wznawia wstrzymane odliczanie; nic nie robi, gdy minutnik nie jest wstrzymany |
| `toggle` (Start, pauza lub wznowienie) | Jeden klawisz dla trzech powyższych |
| `skip` (Pomiń fazę) | Kończy bieżącą fazę od razu i przechodzi do następnej, która biegnie, jeśli minutnik biegł. Pominięta sesja skupienia nie liczy się do statystyk; cykl idzie dalej tak, jakby się skończyła |
| `reset` (Wyzeruj) | `scope`: `phase` (domyślnie) zatrzymuje minutnik i przywraca bieżącej fazie pełną długość; `cycle` wraca do sesji skupienia 1; `statistics` czyści liczniki i nie rusza minutnika; `all` to `cycle` i `statistics` |
| `status` (Stan) | Podaje stan według ostatniego taktu; niczego nie zmienia |

Każda akcja odpowiada komunikatem takim jak `Focus 2 of 4: 12 min left`
(komunikaty akcji są po angielsku) i tymi wyjściami dla kolejnych węzłów
przepływu pracy:

| Wyjście | |
| --- | --- |
| `state` | `idle` (faza nierozpoczęta), `running` (biegnie), `paused` (wstrzymany) |
| `phase` | `focus` (skupienie), `shortBreak` (krótka przerwa), `longBreak` (długa przerwa) |
| `remaining` | Minuty pozostałe do końca fazy |
| `session` | Numer sesji skupienia w cyklu, od 1 |
| `completedToday`, `focusMinutesToday` | Sesje skupienia ukończone w dniu ostatniego taktu i ich minuty |
| `completedTotal`, `focusMinutesTotal` | To samo od instalacji wtyczki lub wyzerowania statystyk |

`tick` dodaje `ended` (`none`, `focus`, `shortBreak`, `longBreak`: faza,
która skończyła się z tym taktem), `notified`, `away` (minutnik został
wstrzymany przez długą pauzę między taktami) oraz, gdy powiadomienia nie
pokazano, `reason`. `start`, `pause`, `resume` i `toggle` dodają `changed`.
`skip` dodaje `skipped`. `status` dodaje `away`, `sessionsPerCycle`,
`phaseMinutes`, `progress` (0-100), `statisticsDate`, `lastTick` oraz
`keyLabel` i `keyDetail`: dwa wiersze, które pokazałby klawisz (niżej).

Te same osiem wartości jest publikowanych jako zmienne dla przepływów
pracy: `{{ plugins.deckpilot.pomodoro.state }}`, `.phase`, `.remaining`,
`.session` po każdej akcji, a `.completedToday`, `.focusMinutesToday`,
`.completedTotal`, `.focusMinutesTotal` po akcjach Takt, Stan i Wyzeruj.
DeckPilot trzyma zmienne wtyczek tylko w pamięci; wracają po pierwszym
takcie.

## Ustawienia

W aplikacji ustawienia mają angielskie nazwy (w nawiasach).

| Ustawienie | Domyślnie | |
| --- | --- | --- |
| Sesja skupienia w minutach (Focus session) | 25 | 1-180 |
| Krótka przerwa w minutach (Short break) | 5 | 1-60 |
| Długa przerwa w minutach (Long break) | 15 | 1-120 |
| Sesje skupienia przed długą przerwą (Focus sessions before a long break) | 4 | 1-12. Przy 1 każda przerwa jest długa |
| Automatycznie uruchamiaj przerwy (Start breaks automatically) | włączone | Wyłączone: przerwa czeka na Start |
| Automatycznie uruchamiaj sesje skupienia (Start focus sessions automatically) | wyłączone | Włączone: następna sesja zaczyna się, gdy kończy się przerwa |
| Nieobecność po, w minutach (Away after) | 5 | 3-120. Tak długa pauza między dwoma taktami wstrzymuje biegnący minutnik |
| Język powiadomień (Language of notifications) | `en` | `en` lub `pl`: wtyczka nie może poznać języka aplikacji |
| Powiadomienia na pulpicie (Desktop notifications) | włączone | DeckPilot pokazuje powiadomienia jako powiadomienia systemu operacyjnego |
| Pasek stanu (Status bar) | włączone | DeckPilot pokazuje je też na swoim pasku stanu, gdy jest otwarty |

Zmieniona długość działa od razu dla fazy, która się nie zaczęła, a dla
fazy, która biegnie lub jest wstrzymana, od następnej fazy.

## Jak się zachowuje

- **Dokładność to jeden takt, czyli minuta.** Zapisany stan to pozostałe
  minuty, a nie termin. Start, wstrzymanie, wznowienie, pominięcie
  i wyzerowanie działają od razu z klawisza, bez znajomości czasu; minuty
  odliczają kolejne takty. Faza uruchomiona między dwoma taktami jest
  więc najwyżej o minutę krótsza niż w ustawieniu, a DeckPilot może
  uruchomić harmonogram do dwóch minut później. Sekundy nie są możliwe:
  czas wyzwalacza ich nie ma.
- **Jeden takt kończy najwyżej jedną fazę.** Gdy minęło więcej czasu, niż
  zostało fazie, faza kończy się raz, a następna zaczyna się z pełną
  długością; nadwyżka nie przechodzi dalej.
- **Po uśpieniu minutnik jest wstrzymany, a nie zakończony.** DeckPilot
  nie nadrabia uruchomień harmonogramu pominiętych, gdy komputer spał,
  a DeckPilot był zablokowany lub wyłączony. Gdy takt przychodzi
  „Away after” minut lub więcej po poprzednim, a minutnik biegł, nic
  z tego czasu nie jest liczone: minutnik zostaje wstrzymany tam, gdzie
  był, mówi o tym jedno powiadomienie, a Wznów (albo Start, albo klawisz
  przełączający) go kontynuuje. Dla minutnika wstrzymanego lub
  nieuruchomionego nic się nie zmienia.
- **Cofnięcie zegara** (ręczne albo koniec czasu letniego) niczego nie
  odejmuje od odliczania; biegnie ono dalej od nowego czasu. Przestawienie
  zegara o godzinę do przodu to długa pauza, jak wyżej.
- **Statystyki** liczą sesję skupienia, gdy jej odliczanie dobiegnie
  końca, z minutami, z którymi sesja się zaczęła. Sesje pominięte
  i wyzerowane się nie liczą. „Dzisiaj” to data ostatniego taktu: pierwszy
  takt innej daty zaczyna liczniki dnia od nowa, a sesja zaczęta przed
  północą liczy się w dniu, w którym się skończyła. Klawisz nie może
  poznać daty, więc po nocy bez taktów Stan pokazuje wczorajsze liczniki
  aż do pierwszego taktu; `statisticsDate` mówi, której daty dotyczą.
- **Powiadomienie odrzucone przez DeckPilot** (więcej niż 5 na minutę z tej
  wtyczki albo błąd) jest ponawiane z kolejnymi taktami przez 5 minut,
  a potem porzucane; `tick` mówi o tym w `reason`. Minutnik i statystyki
  nigdy na nie nie czekają. Powiadomienie, którego Twoje ustawienia nigdzie
  nie pokazują, liczy się jako pokazane.
- **Gdy zawiedzie pamięć wtyczki**, akcja kończy się błędem
  `storage-unavailable`, nic nie jest pokazywane i nic nie ginie:
  następny takt, który uda się zapisać, policzy minuty od ostatniego
  zapisanego.

## Na klawiszu

*Od wersji 0.2.0; wymaga DeckPilota z API wtyczek 1.2.* Klawisz przypisany
do akcji **Stan** pokazuje pozostałe minuty (`12 min`), fazę w drugim
wierszu (`Skupienie 2/4`, `Krotka przerwa`, `Dluga przerwa`, z dopiskiem
`- pauza` lub `- gotowe`; po angielsku, gdy tak mówi ustawienie języka),
pasek postępu fazy, pasek u góry, gdy odliczanie biegnie, oraz kolor fazy:
czerwony dla skupienia, zielony dla krótkiej przerwy, morski dla długiej.
Znacznik w rogu jest niebieski w trakcie odliczania, bursztynowy podczas
pauzy, szary przed startem. Wyjścia `keyLabel`, `keyDetail` i `progress`
akcji Stan mówią to samo słowami.

Klawisz pokazuje minuty **z ostatniego taktu**: minutnik przesuwa się
tylko wtedy, gdy przepływ uruchamia Takt co minutę. Gdy odliczanie
powinno biec, ale od 3 minut nie było taktu (przepływ wyłączony,
komputer uśpiony, DeckPilot zablokowany) albo nie było żadnego, klawisz
nie udaje, że liczy: dodaje znacznik ostrzeżenia i czas od ostatniego
taktu jako plakietkę (`12m`, `2h`), albo `?` przed pierwszym taktem.
Następny takt doprowadza minutnik do porządku jak zawsze.

Teksty na klawiszach są w czcionce DeckPilota, która nie ma polskich
liter (`Krotka przerwa`). Naciśnięcie klawisza uruchamia Stan, który
niczego nie zmienia; **Start, pauza lub wznowienie** umieść na innym
klawiszu albo na długim naciśnięciu tego. Urządzenie bez wyświetlaczy nic
nie pokazuje.

## Uprawnienia i dane

- **Powiadomienia**: żeby pokazać koniec fazy tam, gdzie wskazują dwa
  ustawienia powyżej.
- **Stan klawisza** (`device`, `state`): żeby pokazać odliczanie na
  klawiszach przypisanych do akcji Stan ([Na klawiszu](#na-klawiszu)).
  Wtyczka opisuje klawisz; rysuje go DeckPilot.
- Bez dostępu do sieci, bez poświadczeń.
- Wtyczka trzyma jedną małą wartość we własnej pamięci na tym komputerze:
  fazę, pozostałe minuty, numer sesji, czas ostatniego taktu,
  powiadomienie czekające na pokazanie i liczniki. Nic nie opuszcza
  komputera, a DeckPilot usuwa te dane przy odinstalowaniu wtyczki.
- Do dziennika trafia wiersz, gdy powiadomienie zostało odrzucone lub
  porzucone, gdy długa pauza wstrzymała minutnik, gdy zegar się cofnął
  i gdy zapisanego stanu nie dało się użyć. Żaden wiersz nie zawiera
  parametru ani ustawienia.

## Błędy

| Kod | Znaczenie |
| --- | --- |
| `invalid-fired-at` | Czas przekazany do akcji Takt nie jest datą i godziną wyzwalacza (`RRRR-MM-DD GG:MM`): sprawdź kroki 4 i 5 konfiguracji |
| `storage-unavailable` | Nie udało się odczytać lub zapisać stanu minutnika; akcję można uruchomić ponownie |
| `invalid-parameters`, `not-permitted`, `storage-quota` | Wspólne kody DeckPilot: brak parametru lub wartość spoza zakresu; wtyczka jest wyłączona albo cofnięto jej uprawnienie do powiadomień (minutnik liczy dalej, nie udaje się tylko powiadomienie); pamięć wtyczki jest pełna |

## Czego nie robi

Nie w tej wersji, bo API wtyczek nie ma do tego wywołania:

- **Brak sekund.** Odliczanie ma pełne minuty, a jego dokładność to jeden
  takt.
- **Brak własnego zegara.** Bez przepływu pracy nic się nie odlicza,
  a wtyczka nie może utworzyć go za Ciebie. Jej akcje nie potrafią
  stwierdzić, że takty ustały: klawisz naciśnięty, gdy takty nie
  przychodzą, nadal odpowiada, stanem z ostatniego taktu. Tylko klawisz
  Stanu to zauważa i oznacza ([Na klawiszu](#na-klawiszu)).
- **Brak wykrywania bezczynności.** Wtyczka nie wie, czy jesteś przy
  klawiaturze; za nieobecność liczy się tylko pauza między taktami.
- **Brak dźwięku i przycisków** w powiadomieniu: start i pomijanie to
  klawisze albo węzły przepływu pracy.
- **Brak historii dzień po dniu**: dzisiejsze liczniki i sumy, nic więcej.
- **Jeszcze nie do zainstalowania z Marketplace**: DeckPilot przyjmuje
  wydawcę `deckpilot` tylko we wtyczkach dołączonych do aplikacji
  ([docs/plugin-factory.md](../../docs/plugin-factory.md), B6).

## Budowanie i testy

Zobacz [`first-party/README.md`](../README.md#commands). Minutnik to zestaw
czystych funkcji w `src/lib.rs` (`State::advance` i akcje klawiszy),
testowanych minuta po minucie w `src/tests.rs`.
