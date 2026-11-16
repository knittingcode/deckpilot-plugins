---
title: "Zasady bezpieczeństwa"
description: "Wytyczne bezpieczeństwa dla autorów wtyczek, wynikające z modelu piaskownicy i zasad przeglądu w rejestrze, oraz sposób zgłaszania luk."
group: reference
order: 17
---

DeckPilot traktuje każdą wtyczkę jako kod niezaufany: działa ona w piaskownicy, ma tylko uprawnienia przyznane przez użytkownika i nigdy nie widzi sekretu. Chroni to użytkowników przed wtyczką, ale wtyczka, która zachowuje się poprawnie, wciąż ma coś do zrobienia. Ta strona zbiera wytyczne dla autorów, wynikające z modelu piaskownicy i z tego, co sprawdzają recenzenci rejestru, oraz wyjaśnia, gdzie zgłosić lukę.

> **Status:** Obsługa wtyczek jest eksperymentalna i wyłączona w wydanych wersjach. Ani DeckPilot, ani rejestr wtyczek nie przeszły niezależnego przeglądu bezpieczeństwa i oba repozytoria informują o tym w swoich zasadach bezpieczeństwa.

## Co wymusza platforma

Moduł nie ma własnego systemu plików, sieci, zegara ani procesu. Każdy skutek zewnętrzny jest żądaniem funkcji hosta (capability) sprawdzanym względem aktualnie przyznanych przez użytkownika uprawnień, żądania trafiają wyłącznie pod dokładnie określone adresy `https`, sekrety są wstrzykiwane przez agenta i ukrywane w odpowiedziach, a każde wywołanie działa w ramach limitów czasu, pamięci i liczby wywołań. Model opisuje strona [Architektura](../architecture/). Poniżej znajduje się to, co pozostaje po twojej stronie.

## Zasada najmniejszych uprawnień

- **Deklaruj tylko to, co wywołuje twój kod.** Każde uprawnienie kosztuje użytkownika decyzję o zgodzie, także te rodzaje, które nie mają jeszcze żadnego skutku.
- **Dodawaj uprawnienia późno.** Dodanie lub rozszerzenie uprawnienia w aktualizacji jest eskalacją: użytkownicy są pytani ponownie, a recenzenci traktują ją równie ostrożnie jak nową wtyczkę.
- **Wybieraj brak uprawnień.** Pamięć trwała, ustawienia, własne zmienne wtyczki i logowanie nie wymagają żadnego. Wiele przydatnych akcji to czyste obliczenia.
- **Nie proś o więcej pamięci trwałej** niż domyślne 256 KiB, chyba że jej potrzebujesz.
- **Pisz uczciwe uzasadnienia.** Uzasadnienie jest pokazywane w oknie zgody, a recenzenci porównują je z tym, co wtyczka robi. Napisz konkretnie, jakie dane dokąd trafiają i dlaczego.

## Wąski zakres hostów i metod

- Wymieniaj dokładne hosty należące do usługi, z której twoja wtyczka deklaruje korzystanie. Recenzenci to sprawdzają.
- Wymieniaj tylko te metody HTTP, których używasz. Wtyczka, która tylko odczytuje, deklaruje `GET`.
- Nie deklaruj hostów z symbolami wieloznacznymi. Przechodzą walidację względem schematu, ale nigdy nie są przyznawane, więc jedynie utrudniają przegląd wtyczki.
- Powiąż każde pole danych uwierzytelniających (slot) z najmniejszym zbiorem hostów. Sekret może trafić do każdego hosta swojego slotu.
- Hosty `openUrl` powinny być stronami, które twoje akcje otwierają dla użytkownika, takimi jak twoja usługa lub jej dokumentacja.

## Nie umieszczaj nigdzie sekretów

- Używaj pól danych uwierzytelniających (slotów). Nigdy nie proś użytkownika o wklejenie tokenu do parametru lub ustawienia: to zwykłe wartości, które wtyczka odczytuje, i nie są objęte żadnym z mechanizmów, którymi objęty jest slot (wstrzykiwanie przez agenta, ukrywanie w odpowiedziach).
- Nigdy nie zapisuj sekretu, tokenu ani danych osobowych do logu, danych wyjściowych, komunikatu, zmiennej ani pamięci trwałej.
- Nie umieszczaj w komunikatach pełnych adresów URL z parametrami zapytania. Zapytania często zawierają tokeny.
- Nie kopiuj treści odpowiedzi w całości do danych wyjściowych ani komunikatów o błędach. Wyodrębnij pola, których potrzebujesz.
- Dane wyjściowe można oznaczyć w manifeście jako `sensitive`, podobnie jak zmienne. Używaj tej flagi dla wartości, których użytkownik nie chciałby widzieć na ekranie.

## Traktuj wszystkie dane wejściowe jako wrogie

Parametry docierają zwalidowane względem swoich rodzajów, ale rodzaje nie znają twojej dziedziny, a zdalne odpowiedzi nie są walidowane wcale.

- Waliduj wartości, zanim zbudujesz z nich adres URL. Przykład na tych stronach akceptuje nazwę usługi tylko wtedy, gdy składa się ona z małych liter, cyfr i łączników.
- Sprawdź, czy sparsowana odpowiedź ma oczekiwany kształt i zakresy, zanim jej użyjesz lub ją zapiszesz.
- Ograniczaj to, co przechowujesz: przycinaj ciągi znaków ze zdalnych źródeł do rozsądnej długości, zanim umieścisz je w danych wyjściowych, zmiennych lub pamięci trwałej.
- Nigdy nie panikuj z powodu danych wejściowych. Panika kończy proces hosta, a pięć awarii w ciągu dziesięciu minut wyłącza wtyczkę. Zwróć `PluginError`.

## Bezpieczna obsługa niepowodzeń

- Obsłuż odmowę przy każdym wywołaniu funkcji hosta. Użytkownik może cofnąć uprawnienie między dwoma wywołaniami w tej samej akcji.
- Traktuj opcjonalną funkcję hosta jako opcjonalną. Powiadomienie, którego nie da się pokazać, nie powinno powodować niepowodzenia sprawdzenia, które poza tym się udało.
- Oznaczaj błąd jako możliwy do ponowienia tylko wtedy, gdy ponowienie jest nieszkodliwe, i ustawiaj `retrySafe` dla akcji tylko wtedy, gdy jej dwukrotne uruchomienie nie może wyrządzić szkody.
- Deklaruj `effect` każdej akcji zgodnie z prawdą: `readOnly`, `opens` lub `modifies`.

## Hermetyczne buildy, które da się przejrzeć

Rejestr buduje twoją wtyczkę ponownie ze źródeł, a twoje skrypty budowania i makra proceduralne działają w jego CI. Recenzenci szukają wszystkiego, co niewyjaśnione.

- Przypnij dokładną wersję toolchainu i zacommituj `Cargo.lock`.
- Żadnego skryptu budowania, który cokolwiek pobiera lub wykonuje, i żadnego dostępu do sieci poza zablokowanymi zależnościami Cargo.
- Żadnego zaciemnionego kodu, wygenerowanych blobów ani dołączonych plików binarnych.
- Ogranicz liczbę zależności. Każda musi się budować dla `wasm32-unknown-unknown` bez WASI.
- Nigdy nie przesuwaj ani nie usuwaj tagu, który wskazuje rekord w rejestrze.

## Chroń swoje konto wydawcy

Twoja przestrzeń nazw jest powiązana z twoim kontem GitHub, a zasady bezpieczeństwa rejestru wymieniają przejęte konto wydawcy lub repozytorium źródłowe wśród spraw, które należy zgłaszać. Zabezpiecz konto i repozytorium, na które wskazują twoje rekordy.

## Zgłaszanie luki

Zgłaszaj prywatnie, przez prywatne zgłaszanie luk w serwisie GitHub. Wybór repozytorium zależy od tego, czego dotyczy problem.

| Co | Gdzie |
| --- | --- |
| Złośliwa lub niebezpieczna wtyczka wymieniona w rejestrze albo do niego zgłoszona | Repozytorium rejestru |
| Luka w rejestrze: doprowadzenie do zaakceptowania przez klientów dokumentu lub pakietu bez kluczy rejestru; cofnięcie do starszego stanu, zamrożenie lub ekwiwokacja (equivocation) wykraczające poza udokumentowane granice; obejście odwołania | Repozytorium rejestru |
| Luka w workflow lub narzędziach repozytorium rejestru | Repozytorium rejestru |
| Ujawnienie klucza prywatnego, hasła lub sekretu repozytorium rejestru | Repozytorium rejestru |
| Luka w aplikacji lub agencie DeckPilota, piaskownicy i hoście wtyczek, SDK wtyczek albo w kodzie weryfikacji rejestru po stronie klienta | Repozytorium DeckPilota |

- Repozytorium rejestru: otwórz jego kartę Security i wybierz „Report a vulnerability” albo przejdź do <https://github.com/knittingcode/deckpilot-plugins/security/advisories/new>. Jego zasady stwierdzają, że nie ma osobnego adresu e-mail ani innego kanału.
- Repozytorium DeckPilota: <https://github.com/knittingcode/deckpilot/security/advisories/new>.

Oba zbiory zasad proszą zgłaszających o to samo: nie otwieraj publicznego issue ani pull requesta w sprawie podejrzewanej luki lub złośliwej wtyczki i nie publikuj szczegółów exploita. Zasady rejestru dodają: nie testuj na instalacjach innych użytkowników.

Przydatne zgłoszenie podaje, czego dotyczy problem (identyfikator i wersja wtyczki albo wersja lub commit DeckPilota, których dotyczy problem, oraz system operacyjny), co zaobserwowano, jak to odtworzyć i co zyskuje atakujący. Każdy ze zbiorów zasad wymienia, o co prosi.

W przypadku problemu z wtyczką, który nie jest kwestią bezpieczeństwa, takiego jak awaria lub mylący opis, użyj formularza issue *Plugin report* w rejestrze.

### Co mówią zasady

Zasady rejestru nie podają gwarantowanych czasów odpowiedzi i stwierdzają, że zgłoszenia są obsługiwane przed jakąkolwiek inną pracą. Dla potwierdzonej złośliwej lub niebezpiecznej wtyczki opisują odwołanie wtyczki w podpisanym indeksie i przegląd pozostałych wtyczek wydawcy. Zasady DeckPilota wymieniają potwierdzenie otrzymania zgłoszenia w ciągu 7 dni jako cel, a nie gwarancję, i proszą zgłaszających o danie rozsądnego czasu na naprawę problemu przed jego ujawnieniem. Oba zbiory zasad przewidują wymienienie zgłaszających w komunikacie bezpieczeństwa, chyba że wolą oni inaczej.

Wiążące są same zasady: [SECURITY.md rejestru](https://github.com/knittingcode/deckpilot-plugins/blob/main/SECURITY.md) i [SECURITY.md DeckPilota](https://github.com/knittingcode/deckpilot/blob/main/SECURITY.md).

> **Uwaga:** W chwili pisania tego tekstu sekcja zakresu w zasadach DeckPilota wciąż opisuje wtyczki jako propozycję projektową. Zasady rejestru kierują zgłoszenia dotyczące piaskownicy, hosta, SDK i weryfikacji po stronie klienta do repozytorium DeckPilota.

## Dokumentacja źródłowa

[Zasady przeglądu](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/review-policy.md) i [model zaufania](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/trust-model.md) rejestru oraz [reguły uprawnień](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/permissions.md) DeckPilota.
