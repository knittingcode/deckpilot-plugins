---
title: "Cykl życia"
description: "Co i kiedy się uruchamia: haki init i shutdown, proces hosta, limity wywołania, anulowanie, awarie i restarty oraz to, czego brakuje w ABI 1."
group: build
order: 11
---

Wtyczka nie jest programem działającym w sposób ciągły. Ma dwa opcjonalne haki, init i shutdown, z których żaden nie może korzystać z funkcji hosta (capability), a poza nimi jej kod działa tylko wtedy, gdy wywoływana jest jedna z jej akcji. Ta strona opisuje proces hosta otaczający moduł, limity jednego wywołania oraz to, co się dzieje, gdy coś pójdzie nie tak.

## Proces hosta i moduł

- Agent uruchamia jeden proces hosta w piaskownicy dla każdej włączonej wtyczki **na żądanie**, przy pierwszym wywołaniu, a nie w chwili włączenia wtyczki.
- Przed każdym uruchomieniem agent weryfikuje zainstalowane pliki względem ich rekordu integralności i ponownie waliduje `plugin.wasm`.
- Host tworzy instancję modułu raz i utrzymuje ją przez cały czas życia procesu. Zmienne statyczne oraz `Plugin` zbudowany przez makro `plugin!` są zachowywane między wywołaniami tego hosta, dopóki się nie zatrzyma.
- Nie polegaj na stanie w pamięci operacyjnej w przypadku niczego, co musi przetrwać. Użyj [pamięci trwałej](../storage/).
- Wywołania jednej wtyczki wykonują się pojedynczo, po kolei. W kolejce może czekać do 8 wywołań; kolejne od razu kończą się niepowodzeniem z powodu zajętości. Różne wtyczki działają równolegle.

## Haki

Zarejestruj haki w definicji wtyczki za pomocą `.on_init(init)` i `.on_shutdown(shutdown)`:

```rust
/// Runs once when the host loads the module. No capabilities here.
fn init() -> Result<(), PluginError> {
    if API.starts_with("https://") {
        Ok(())
    } else {
        Err(PluginError::new(codes::PLUGIN_ERROR, "the API origin must be https"))
    }
}

/// Runs on a clean stop only. No capabilities, best effort.
fn shutdown() {}
```

| Hak | Sygnatura | Kiedy | Co może zrobić |
| --- | --- | --- | --- |
| init | `fn() -> Result<(), PluginError>` | Raz, gdy host ładuje moduł, przed pierwszym wywołaniem | Sprawdzić statyczne tablice, przygotować stan w pamięci operacyjnej |
| shutdown | `fn()` | Przy czystym zatrzymaniu: wyłączenie, aktualizacja, przywrócenie poprzedniej wersji, odinstalowanie, kwarantanna, zablokowanie i zatrzymanie agenta | Posprzątać stan w pamięci operacyjnej, w miarę możliwości |

Zasady dla obu haków:

- **Żadnych funkcji hosta.** Hak nie ma `Context`. Nie istnieje jeszcze żadne wywołanie, więc hak nie może pobierać danych z sieci, zapisywać, publikować ani logować.
- Działają w ramach tych samych limitów paliwa (fuel) i procesora co wywołanie.
- Nieudany init odrzuca załadowanie: wywołanie, które go spowodowało, kończy się niepowodzeniem, a następne próbuje z nowym hostem.
- Shutdown nie jest uruchamiany po awarii, przekroczeniu limitu czasu ani po zabiciu procesu. Po tym, jak host dostanie polecenie zakończenia pracy, jest zabijany po 1 sekundzie.

Ponieważ haki nie mają funkcji hosta, typowa praca przy starcie, taka jak „wczytaj mój stan”, powinna się znaleźć na początku akcji i polegać na odczycie z pamięci trwałej.

## Jedno wywołanie

1. Agent sprawdza stan wtyczki, platformę i parametry. Błędne parametry kończą się błędem `invalid-parameters`, zanim host zostanie uruchomiony.
2. Host przekazuje dane wejściowe do modułu. SDK uruchamia init, jeśli to potrzebne, znajduje akcję, sprawdza parametry, wywołuje procedurę obsługi i sprawdza wynik.
3. Każde wywołanie funkcji hosta przechodzi synchronicznie od hosta do brokera uprawnień i wykonawcy w agencie, a potem z powrotem. Moduł czeka.

### Limity jednego wywołania

| Limit | Wartość | Po przekroczeniu |
| --- | --- | --- |
| Czas rzeczywisty | `node.timeoutMs` akcji: domyślnie 30 s, najwyżej 10 min | Host jest zabijany; akcja kończy się niepowodzeniem |
| Czas procesora między dwoma wywołaniami hosta | 250 ms | Pułapka (trap); host kończy pracę |
| Paliwo (fuel) | 5 000 000 000 na wywołanie, w przybliżeniu liczba instrukcji | Pułapka (trap); host kończy pracę |
| Pamięć liniowa | 64 MiB | Alokacja kończy się niepowodzeniem albo pułapką (trap) |
| Stos WebAssembly | 512 KiB | Pułapka (trap) |
| Wywołania funkcji hosta | 256 | SDK odrzuca kolejne wywołania jako ograniczone (throttled) |
| Dane wejściowe i wyjściowe | 1 MiB i 192 KiB w postaci JSON | Odrzucane |

### Anulowanie

Anulowane uruchomienie, na przykład anulowany workflow, albo uruchomienie po przekroczeniu terminu natychmiast zatrzymuje trwające wywołanie funkcji hosta. Późniejsze wywołania są odrzucane, a wynik wywołania akcji jest pomijany: akcja kończy się niepowodzeniem jako anulowana. ABI 1 nie ma komunikatu przerwania dla modułu, więc host dostaje 2 sekundy na dokończenie pracy, po czym jest zabijany. Nie liczy się to jako awaria.

Pisz procedury obsługi tak, aby wykonywały wywołania funkcji hosta wcześnie i szybko kończyły pracę po nieudanym wywołaniu.

### Cofnięcie uprawnienia w trakcie uruchomienia

Przyznane uprawnienia są odczytywane przy każdym wywołaniu funkcji hosta. Cofnięcie uprawnienia, wyłączenie lub odinstalowanie wtyczki sprawia, że następne wywołanie funkcji hosta w trwającym wywołaniu akcji kończy się błędem `not-permitted`. Wywołanie, które już trwa, nie jest przerywane.

## Awarie i ponowne uruchomienia

Panika przerywa działanie modułu, ponieważ profil budowania `guest` ustawia `panic = "abort"`. Proces hosta kończy pracę, a akcja kończy się błędem `plugin-host-failed`.

- Następne wywołanie uruchamia nowego hosta, z opóźnieniem 1, 2, 4 i maksymalnie 60 sekund.
- Pięć awarii w ciągu dziesięciu minut wyłącza wtyczkę. To bezpiecznik chroniący przed pętlą awarii.

Zwracaj `PluginError` zamiast panikować i unikaj `unwrap` na wszystkim, na co mogą wpłynąć dane wejściowe.

## Stan a zdarzenia

| Zdarzenie | Host | Pamięć trwała | Ustawienia i dane uwierzytelniające | Opublikowane zmienne | Przyznane uprawnienia |
| --- | --- | --- | --- | --- | --- |
| Ponowne uruchomienie agenta, zablokowanie i odblokowanie | zatrzymany | zachowana | zachowane | usunięte | zachowane |
| Wyłączenie i włączenie | zatrzymany | zachowana | zachowane | usunięte | zachowane, nieaktywne, gdy wtyczka jest wyłączona |
| Cofnięcie lub przyznanie uprawnienia | działa | zachowana | zachowane | zachowane | obowiązuje od następnego wywołania |
| Aktualizacja, przywrócenie poprzedniej wersji | zatrzymany | zachowana | zachowane | usunięte | eskalacje wymagają zgody |
| Awaria hosta | uruchamiany ponownie na żądanie | zachowana | zachowane | zachowane | zachowane |
| Odinstalowanie | zatrzymany | usunięta | usunięte, chyba że użytkownik je zachowa | usunięte | usunięte |

## Czego nie ma w ABI 1

Pierwotny projekt i deklaracje typów JavaScript opisują więcej, niż implementuje ABI 1. Mówiąc wprost:

- **Brak pracy w tle.** Żadnych timerów ani interwałów. Uprawnienie `background` istnieje w schemacie manifestu, ale nie ma żadnego skutku. Wtyczka działa tylko wtedy, gdy działa jedna z jej akcji.
- **Brak haków aktywacji z funkcjami hosta.** Init i shutdown to jedyne haki i nie mogą wywoływać funkcji hosta.
- **Brak powiadomienia o zmianie ustawień.** Odczytuj `Context::settings()` w każdym wywołaniu.
- **Brak stanu przycisku.** Pole `state` manifestu przechodzi walidację, ale nic nie pobiera stanu z wtyczki. Nie ma dostępu do urządzenia.
- **Brak zegara i brak uśpienia.** Moduł nie może odczytać czasu ani czekać.
- **Brak raportowania postępu** z działającej akcji.

Jedno, czego procedura obsługi może się dowiedzieć o swoim wywołaniu, to co je rozpoczęło: `Context::run_context()` zwraca informację, czy uruchomienie rozpoczął przycisk, gest, aplikacja czy workflow, a w przypadku workflow, czy zrobił to użytkownik, harmonogram czy zdarzenie. Nie udostępnia terminu ani numeru próby.

```rust
    // Pages open only in a run the user started; otherwise report the link.
    if ctx.run_context()?.is_user_initiated() {
        ctx.open_url(url.clone())?;
        Ok(Output::message("opened the status page").with("url", url))
    } else {
        Ok(Output::message("not opened: this run was not started by the user").with("url", url))
    }
```

## Dokumentacja źródłowa

[docs/marketplace/sdk/lifecycle.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/lifecycle.md). Jego końcowa lista wciąż podaje, że procedura obsługi nie dowiaduje się, co ją uruchomiło; kontekst uruchomienia opisany w [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md) został dodany później i jest zaimplementowany w SDK oraz w agencie.
