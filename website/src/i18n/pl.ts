import type { Dictionary } from './index';

/** Polish. Must have exactly the keys of `en` (enforced by the type). */
export const pl: Dictionary = {
  site: {
    name: 'DeckPilot Plugin Marketplace',
    shortName: 'Marketplace',
    tagline: 'Wtyczki do DeckPilota, otwartoźródłowego kontrolera Stream Decka.',
    description:
      'DeckPilot Plugin Marketplace: niezależnie instalowane wtyczki rozszerzające DeckPilota, otwartoźródłowy, działający lokalnie kontroler Stream Decka. Podpisany rejestr, wtyczki w piaskownicy, jawne uprawnienia.',
  },

  a11y: {
    skipToContent: 'Przejdź do treści',
    mainNav: 'Główna',
    footerNav: 'Stopka',
    openMenu: 'Otwórz menu',
    closeMenu: 'Zamknij menu',
    homeLink: 'DeckPilot Plugin Marketplace, strona główna',
    languageSwitcher: 'Język',
    switchToDark: 'Włącz ciemny motyw',
    switchToLight: 'Włącz jasny motyw',
    externalLink: '(otwiera się w serwisie GitHub)',
    breadcrumb: 'Ścieżka nawigacji',
    table: 'Tabela',
    copyCode: 'Kopiuj kod',
    copied: 'Skopiowano',
    copyFailed: 'Nie udało się skopiować',
  },

  nav: {
    plugins: 'Wtyczki',
    developers: 'Dla twórców',
    docs: 'Dokumentacja',
    community: 'Społeczność',
    github: 'GitHub',
  },

  status: {
    badge: 'Rejestr działa',
    short: 'Podpisany rejestr wtyczek działa. DeckPilot 0.10.0, bieżące wydanie, instaluje wtyczki ze swojego Marketplace; pierwszym wydaniem z obsługą wtyczek był 0.7.0.',
  },

  footer: {
    about:
      'Społecznościowy rejestr wtyczek DeckPilota, otwartoźródłowej, działającej lokalnie aplikacji do sterowania Stream Deckiem i automatyzacji pracy.',
    marketplace: 'Marketplace',
    developers: 'Dla twórców',
    project: 'Projekt',
    explore: 'Przeglądaj wtyczki',
    howItWorks: 'Jak to działa',
    securityModel: 'Bezpieczeństwo i uprawnienia',
    faq: 'Pytania i odpowiedzi',
    portal: 'Portal dla twórców',
    docs: 'Dokumentacja',
    gettingStarted: 'Pierwsze kroki',
    submit: 'Zgłoś wtyczkę',
    website: 'Strona DeckPilot',
    deckpilot: 'DeckPilot w serwisie GitHub',
    registryRepo: 'Repozytorium rejestru',
    contributing: 'Zasady współpracy',
    securityPolicy: 'Polityka bezpieczeństwa',
    codeOfConduct: 'Kodeks postępowania',
    license: 'Licencja',
    legal:
      'Ta witryna i narzędzia rejestru są udostępniane na licencji Apache-2.0. Każda wtyczka ma własną licencję.',
    trademark:
      'Stream Deck jest znakiem towarowym swojego właściciela. DeckPilot to niezależny projekt open source, niepowiązany z firmami Elgato i Corsair ani przez nie niewspierany.',
  },

  home: {
    title: 'DeckPilot Plugin Marketplace: Twój Stream Deck, Twoje możliwości',
    description:
      'Rozszerz DeckPilota o niezależnie instalowane wtyczki: podpisany rejestr, piaskownica WebAssembly, jawne uprawnienia. Dostępne od DeckPilota 0.7.0.',
    hero: {
      headline: 'Twój Stream Deck. Twoje możliwości.',
      lead: 'Marketplace rozszerza DeckPilota o niezależnie instalowane wtyczki: nowe akcje dla usług i narzędzi, z których korzystasz. Każda wtyczka działa w piaskownicy i ma tylko te uprawnienia, na które się zgodzisz.',
      explore: 'Przeglądaj wtyczki',
      build: 'Zbuduj wtyczkę',
      github: 'Zobacz DeckPilota w serwisie GitHub',
      deckAlt: 'Ilustracja siatki przycisków Stream Decka z akcjami wtyczek',
    },
    search: {
      title: 'Znajdź wtyczkę',
      lead: 'Przeszukaj rejestr po nazwie lub opisie.',
      label: 'Szukaj wtyczek',
      placeholder: 'Szukaj wtyczek po nazwie lub opisie',
      submit: 'Szukaj',
      emptyHint: 'Ta wersja witryny nie zawiera żadnych wtyczek, więc wyszukiwarka nie ma czego zwrócić.',
    },
    featured: {
      eyebrow: 'Integracje',
      title: 'Wyróżnione integracje',
      lead: 'Wtyczki ostatnio opublikowane w rejestrze.',
      viewAll: 'Zobacz wszystkie wtyczki',
      emptyTitle: 'Ta wersja witryny nie zawiera wtyczek',
      emptyText:
        'Katalog jest generowany z podpisanego rejestru podczas wdrażania witryny. Ta wersja nie zawiera żadnej wtyczki i niczego nie wymyślamy, żeby zapełnić to miejsce.',
      emptyPrimary: 'Zbuduj wtyczkę',
      emptySecondary: 'Obserwuj rejestr w serwisie GitHub',
    },
    categories: {
      eyebrow: 'Kategorie',
      title: 'Kategorie wtyczek',
      lead: 'Każda wtyczka deklaruje najwyżej trzy kategorie z tej zamkniętej listy, dzięki czemu katalog łatwo się przegląda.',
      count: {
        one: '{count} wtyczka',
        few: '{count} wtyczki',
        many: '{count} wtyczek',
        other: '{count} wtyczki',
      },
    },
    how: {
      eyebrow: 'Jak to działa',
      title: 'Od pull requesta do przycisku na Twoim decku',
      lead: 'Marketplace to podpisany, statyczny rejestr. Nic nie trafia do DeckPilota bez przeglądu, przebudowania ze źródeł i podpisu, który aplikacja sprawdza samodzielnie.',
      steps: [
        {
          title: 'Autor zgłasza wtyczkę',
          text: 'Wtyczka trafia do rejestru jako pull request, który wskazuje dokładny tag i commit w publicznym repozytorium z jej kodem.',
        },
        {
          title: 'CI buduje, właściciel sprawdza',
          text: 'Rejestr buduje pakiet z tych źródeł od nowa i porównuje go bajt po bajcie. Właściciel repozytorium sprawdza kod, uprawnienia i opis wtyczki.',
        },
        {
          title: 'Rejestr zostaje podpisany',
          text: 'Publikacja podpisuje nowy indeks, w którym zapisane są skrót sha256 i rozmiar każdego pakietu. Samo scalenie pull requesta niczego nie publikuje.',
        },
        {
          title: 'DeckPilot weryfikuje i pyta Ciebie',
          text: 'Aplikacja sprawdza podpisy kluczami wbudowanymi w program i pokazuje uprawnienia, o które prosi wtyczka. Wtyczka pozostaje wyłączona, dopóki ich nie przyznasz.',
        },
      ],
      more: 'Poznaj model zaufania',
    },
    security: {
      eyebrow: 'Bezpieczeństwo i uprawnienia',
      title: 'Wtyczka dostaje tylko to, na co pozwolisz',
      lead: 'Wtyczka to moduł WebAssembly uruchamiany w odizolowanym procesie. Nie ma własnego dostępu do plików, sieci, zegara ani zmiennych środowiskowych.',
      points: [
        {
          title: 'Domyślnie w piaskownicy',
          text: 'Każda wtyczka działa w osobnym procesie, izolowanym przez system operacyjny, bez WASI. Wszystko, po co sięga na zewnątrz, jest żądaniem, które DeckPilot sprawdza przy każdym wywołaniu.',
        },
        {
          title: 'Jawne uprawnienia',
          text: 'Hosty sieciowe, dane uwierzytelniające, powiadomienia i pozostałe uprawnienia są zadeklarowane w manifeście, każde z uzasadnieniem. Widzisz je, zanim włączysz wtyczkę, a gdy aktualizacja doda nowe, DeckPilot zapyta ponownie.',
        },
        {
          title: 'Sekrety zostają w DeckPilocie',
          text: 'Wtyczka odwołuje się do danych uwierzytelniających przez uchwyt. DeckPilot dołącza sekret do zatwierdzonego żądania, a wtyczka nigdy nie poznaje jego wartości.',
        },
        {
          title: 'Podpisane i odwoływalne',
          text: 'DeckPilot ufa rejestrowi wyłącznie na podstawie podpisów, które sam weryfikuje. Szkodliwą wtyczkę można odwołać, a aplikacje zatrzymają ją przy najbliższym odświeżeniu rejestru.',
        },
      ],
      caveat:
        'Przegląd i piaskownica zmniejszają ryzyko, ale niczego nie gwarantują. Rejestr nie przeszedł jeszcze niezależnego audytu bezpieczeństwa.',
      more: 'Jak działają uprawnienia',
      report: 'Zgłoś problem z bezpieczeństwem',
    },
    build: {
      eyebrow: 'Dla twórców',
      title: 'Zbuduj własną wtyczkę',
      lead: 'Wtyczki pisze się w języku Rust z użyciem SDK wtyczek DeckPilota i kompiluje do jednego modułu WebAssembly. Oto kompletna wtyczka z jedną akcją:',
      codeTitle: 'src/lib.rs',
      points: [
        'Typowane akcje, parametry i wyniki',
        'Testy jednostkowe z atrapą hosta, uruchamiane zwykłym cargo test',
        'Pośredniczony HTTPS, dane uwierzytelniające, pamięć trwała, zmienne i powiadomienia',
      ],
      note: 'SDK nie jest jeszcze opublikowane w crates.io. Wtyczkę buduje się na podstawie kopii repozytorium DeckPilota.',
      primary: 'Zacznij budować',
      secondary: 'Zobacz przykłady',
    },
    community: {
      eyebrow: 'Open source',
      title: 'Tworzone otwarcie',
      lead: 'DeckPilot i jego rejestr wtyczek są otwartym oprogramowaniem na licencji Apache-2.0. Format, kontrole i proces publikacji są jawne, tak samo jak każdy przegląd.',
      cards: [
        {
          title: 'Zgłoś wtyczkę',
          text: 'Zgłoszenie możesz przygotować i sprawdzić już dziś. Na razie rejestr publikuje własne wtyczki DeckPilota; pozostałe zgłoszenia zostaną przejrzane i opublikowane, gdy rejestr otworzy się dla innych wydawców.',
          link: 'Jak wygląda zgłoszenie',
        },
        {
          title: 'Ulepsz rejestr',
          text: 'Poprawki dokumentacji są mile widziane jako pull requesty. Problemy z narzędziami zgłasza się jako issue.',
          link: 'Przeczytaj zasady współpracy',
        },
        {
          title: 'Zgłoś problem',
          text: 'Złośliwą wtyczkę lub lukę zgłoś prywatnie przez GitHub. Nie ma innego kanału.',
          link: 'Przeczytaj politykę bezpieczeństwa',
        },
      ],
    },
    faq: {
      eyebrow: 'FAQ',
      title: 'Pytania i odpowiedzi',
      items: [
        {
          q: 'Czym jest DeckPilot Plugin Marketplace?',
          a: 'To społecznościowy rejestr wtyczek DeckPilota. Wtyczka dodaje do DeckPilota nowe akcje i jest instalowana niezależnie od aplikacji, z podpisanego rejestru, który aplikacja weryfikuje samodzielnie.',
        },
        {
          q: 'Czy mogę już instalować wtyczki?',
          a: 'Tak, w DeckPilocie 0.7.0, pierwszym wydaniu z obsługą wtyczek, lub nowszym; bieżące wydanie to 0.10.0. Wtyczki są domyślnie włączone. Wtyczki wymienione na tej witrynie są opublikowane w podpisanym rejestrze, a DeckPilot instaluje je ze swojego Marketplace. Git, Docker, Workspace, Projects, GitHub, GitLab, Jira i Workflows oraz powiadomienia DeckPilota to własne wtyczki DeckPilota dołączone do aplikacji. DeckPilot 0.6.0 nie obsługuje wtyczek.',
        },
        {
          q: 'Jak wygląda instalacja wtyczki?',
          a: 'Wtyczki instaluje się z Marketplace w DeckPilocie. Od DeckPilota 0.8.0 otwiera się on z paska narzędzi głównego okna; w wersji 0.10.0 zainstalowanymi wtyczkami można też zarządzać w Settings › Plugins, a w wersji 0.7.0 Marketplace znajduje się w Settings › Extensions › Plugins. Wybierasz wtyczkę, przeglądasz uprawnienia, o które prosi, i zatwierdzasz instalację. Ta witryna jest katalogiem i dokumentacją: nie ma tu przycisku instalacji, a DeckPilot nie obsługuje linków instalacyjnych.',
        },
        {
          q: 'Jak sprawdzane są wtyczki?',
          a: 'Każde zgłoszenie jest pull requestem. Automatyczne kontrole budują pakiet od nowa z otagowanych źródeł autora i porównują go bajt po bajcie, a właściciel repozytorium sprawdza tożsamość wydawcy, kod, każde uprawnienie wraz z uzasadnieniem, sposób budowania i licencję. Zielony wynik kontroli nie oznacza akceptacji, a scalenie niczego nie publikuje.',
        },
        {
          q: 'Do czego wtyczka ma dostęp na moim komputerze?',
          a: 'Tylko do tego, co deklaruje jej manifest i na co się zgodzisz. Wtyczka nie ma bezpośredniego dostępu do plików, sieci ani Twoich sekretów: prosi o nie DeckPilota, który sprawdza każde żądanie pod kątem przyznanych uprawnień. W obecnym ABI wtyczek są to żądania HTTPS do wskazanych hostów, dane uwierzytelniające używane przez uchwyty, otwieranie zatwierdzonych linków w przeglądarce, powiadomienia oraz własne ustawienia, zmienne i pamięć wtyczki.',
        },
        {
          q: 'W jakim języku pisze się wtyczki?',
          a: 'W języku Rust. Kod kompiluje się do modułu WebAssembly z użyciem SDK wtyczek DeckPilota. SDK dla JavaScriptu istnieje wyłącznie jako deklaracje typów: wtyczek w JavaScripcie nie da się jeszcze zbudować ani uruchomić.',
        },
        {
          q: 'Na jakiej licencji są wtyczki?',
          a: 'Każda wtyczka ma własną licencję, zadeklarowaną w manifeście jako wyrażenie SPDX i dołączoną do pakietu. Rejestr i ta witryna są udostępniane na licencji Apache-2.0, która nie obejmuje wtyczek znajdujących się w rejestrze.',
        },
        {
          q: 'Jak zgłosić złośliwą wtyczkę?',
          a: 'Prywatnie, przez prywatne zgłaszanie luk w serwisie GitHub, w repozytorium rejestru. Podejrzenia złośliwej wtyczki lub luki nie zgłaszaj w publicznym issue.',
        },
      ],
    },
  },

  categories: {
    'developer-tools': {
      name: 'Narzędzia programistyczne',
      text: 'Repozytoria, buildy, pull requesty i narzędzia wokół Twojego kodu.',
    },
    productivity: {
      name: 'Produktywność',
      text: 'Zadania, notatki, minutniki i codzienne skróty.',
    },
    communication: {
      name: 'Komunikacja',
      text: 'Czaty, rozmowy, statusy i powiadomienia.',
    },
    monitoring: {
      name: 'Monitoring',
      text: 'Stan usług, metryki i alerty w zasięgu wzroku.',
    },
    media: {
      name: 'Multimedia',
      text: 'Dźwięk, wideo, streaming i sterowanie odtwarzaniem.',
    },
    'smart-home': {
      name: 'Inteligentny dom',
      text: 'Oświetlenie, sceny, czujniki i podłączone urządzenia.',
    },
    other: {
      name: 'Inne',
      text: 'Wszystko, co nie pasuje do żadnej z powyższych kategorii.',
    },
  },

  fixture: {
    banner: 'Wersja testowa z danymi przykładowymi. Te wtyczki nie są prawdziwe i nie zostały opublikowane.',
  },

  directory: {
    title: 'Wtyczki',
    metaTitle: 'Katalog wtyczek',
    description:
      'Przeglądaj wtyczki opublikowane w rejestrze DeckPilota. Szukaj po nazwie lub opisie i filtruj według kategorii, platformy oraz wersji DeckPilota.',
    lead: 'Wszystkie wtyczki opublikowane w podpisanym rejestrze DeckPilota. Lista powstaje bezpośrednio z rejestru.',
    searchLabel: 'Szukaj',
    searchPlaceholder: 'Nazwa lub opis',
    category: 'Kategoria',
    platform: 'Platforma',
    compatibility: 'Wersja DeckPilota',
    anyCategory: 'Wszystkie kategorie',
    anyPlatform: 'Wszystkie platformy',
    anyVersion: 'Dowolna wersja DeckPilota',
    versionOption: 'DeckPilot {version}',
    clear: 'Wyczyść filtry',
    results: {
      one: '{count} wtyczka',
      few: '{count} wtyczki',
      many: '{count} wtyczek',
      other: '{count} wtyczki',
    },
    resultsOf: 'Wyświetlono {shown} z {total}',
    listLabel: 'Wtyczki',
    noResultsTitle: 'Żadna wtyczka nie pasuje do tych filtrów',
    noResultsText: 'Spróbuj wpisać inne hasło albo usuń któryś filtr.',
    emptyTitle: 'Ta wersja witryny nie zawiera wtyczek',
    emptyText:
      'Katalog jest generowany z podpisanego rejestru podczas wdrażania witryny, a ta wersja nie zawiera żadnej wtyczki. Na opublikowanej witrynie pojawia się tu każda wtyczka, którą oferuje rejestr.',
    emptyPrimary: 'Zbuduj wtyczkę',
    emptySecondary: 'Jak wygląda zgłoszenie',
    emptyStatusTitle: 'Na jakim etapie jesteśmy',
    emptyStatus: [
      'Podpisany rejestr produkcyjny działa, a jego znacznik czasu jest codziennie podpisywany na nowo.',
      'Publikuje własne wtyczki DeckPilota. Nie jest jeszcze otwarty dla innych wydawców.',
      'DeckPilot 0.7.0 jest pierwszym wydaniem z obsługą wtyczek, a 0.10.0 to bieżące wydanie. DeckPilot 0.6.0 nie obsługuje wtyczek.',
    ],
    emptyStatusLink: 'Co zostało do zrobienia przed otwarciem rejestru dla innych wydawców',
    errorTitle: 'Wyszukiwanie jest niedostępne',
    errorText: 'Nie udało się wczytać indeksu wyszukiwania, dlatego widoczne są wszystkie wtyczki. Odśwież stronę, aby spróbować ponownie.',
    registryInfo: 'Indeks rejestru nr {seq}, wygenerowany {date}.',
  },

  plugin: {
    metaTitle: '{name}: wtyczka do DeckPilota',
    verifiedPublisher: 'Zweryfikowany wydawca',
    prerelease: 'Wersja wstępna',
    deprecated: 'Wycofywana',
    revoked: 'Odwołana',
    yanked: 'Wycofana wersja',
    by: 'wydawca: {publisher}',
    allPlugins: 'Wtyczki',
    about: 'O wtyczce',
    untranslatedNote: 'Teksty dostarczone przez wydawcę są wyświetlane po angielsku.',
    actions: 'Akcje',
    actionsLead: 'Akcje, które ta wtyczka dodaje do DeckPilota.',
    actionsUnavailable:
      'Lista akcji jest zapisana w pakiecie wtyczki, a żadna sprawdzona kopia manifestu nie odpowiada opublikowanej wersji, dlatego nie jest tu wyświetlana. DeckPilot pokazuje akcje przed instalacją.',
    noActions: 'Ta wersja nie deklaruje żadnych akcji.',
    parameters: 'Parametry',
    outputs: 'Wyniki',
    required: 'wymagany',
    optional: 'opcjonalny',
    deprecatedAction: 'wycofywana',
    permissions: 'Wymagane uprawnienia',
    permissionsLead:
      'DeckPilot pokazuje je przed włączeniem wtyczki, a wtyczka pozostaje wyłączona, dopóki ich nie przyznasz. Każde uprawnienie możesz w dowolnej chwili cofnąć.',
    noPermissions: 'Ta wtyczka nie prosi o żadne uprawnienia. Jej akcje działają w całości wewnątrz piaskownicy.',
    install: 'Jak zainstalować',
    installSteps: [
      'Zainstaluj DeckPilota w wersji 0.7.0 lub nowszej; bieżące wydanie to 0.10.0. Wersja 0.7.0 jest pierwszą z obsługą wtyczek, a wtyczki są w niej domyślnie włączone.',
      'Otwórz Marketplace w DeckPilocie i znajdź wtyczkę {name}. Od DeckPilota 0.8.0 Marketplace otwiera się z paska narzędzi; w wersji 0.7.0 znajduje się w Settings › Extensions › Plugins.',
      'Przejrzyj wymagane uprawnienia i zainstaluj wtyczkę. Przed instalacją DeckPilot weryfikuje podpisy rejestru oraz skrót sha256 i rozmiar pakietu.',
      'Włącz wtyczkę i przyznaj jej uprawnienia. Wtyczka jest instalowana jako wyłączona i do tego momentu nie może nic zrobić.',
    ],
    installNote:
      'Na tej stronie nie ma przycisku instalacji: DeckPilot nie obsługuje linków instalacyjnych, a wtyczki instaluje się wyłącznie z poziomu aplikacji.',
    installRevoked: 'Ta wtyczka została odwołana. DeckPilot nie oferuje jej do instalacji.',
    history: 'Historia wersji',
    historyLead:
      'Wszystkie opublikowane wersje, od najnowszej. Rejestr przechowuje wersje i ich źródła, a nie opisy zmian. Tych szukaj w repozytorium z kodem.',
    version: 'Wersja',
    published: 'Opublikowano',
    source: 'Źródło',
    changes: 'Zmiany',
    compare: 'Porównaj z {version}',
    firstVersion: 'Pierwsza wersja',
    sourceAt: '{tag}, commit {commit}',
    details: 'Szczegóły',
    latestVersion: 'Najnowsza wersja',
    platforms: 'Platformy',
    allPlatforms: 'macOS, Windows i Linux',
    compatibility: 'Wersje DeckPilota',
    compatibleWith: 'Obejmuje bieżące wydanie {version}.',
    notCompatibleWith: 'Nie obejmuje bieżącego wydania {version}.',
    pluginApi: 'API wtyczek',
    license: 'Licencja',
    publisher: 'Wydawca',
    sourceRepository: 'Repozytorium z kodem',
    homepage: 'Strona domowa',
    package: 'Pakiet',
    id: 'Identyfikator wtyczki',
    revokedTitle: 'Ta wtyczka została odwołana',
    revokedDisable: 'DeckPilot zatrzymuje zainstalowane kopie i odmawia ich wczytania.',
    revokedYank: 'Nie jest już oferowana do instalacji ani aktualizacji. Zainstalowane kopie działają dalej.',
    revokedVersions: 'Odwołane wersje: {versions}.',
    reason: 'Powód podany przez rejestr:',
    advisory: 'Przeczytaj komunikat bezpieczeństwa',
    deprecatedTitle: 'Ta wtyczka jest wycofywana',
    replacement: 'Sugerowany zamiennik:',
    report: 'Zgłoś problem z tą wtyczką',
  },

  platforms: { macos: 'macOS', windows: 'Windows', linux: 'Linux' },

  permissions: {
    network: {
      name: 'Dostęp do sieci',
      text: 'Wysyła za pośrednictwem DeckPilota żądania HTTPS do hostów wymienionych poniżej i do żadnego innego serwera.',
    },
    credentials: {
      name: 'Dane uwierzytelniające',
      text: 'Używa sekretów wprowadzonych przez Ciebie w DeckPilocie (na przykład tokenu API) w żądaniach do zatwierdzonych hostów. Sama wtyczka nigdy nie widzi sekretu.',
    },
    notifications: {
      name: 'Powiadomienia',
      text: 'Wyświetla powiadomienia systemowe za pośrednictwem DeckPilota.',
    },
    storage: {
      name: 'Większa pamięć',
      text: 'Podnosi limit prywatnej pamięci wtyczki powyżej domyślnych 256 KiB. Nie daje dostępu do Twoich plików.',
    },
    openUrl: {
      name: 'Otwieranie linków',
      text: 'Otwiera w przeglądarce strony HTTPS na hostach wymienionych poniżej i tylko wtedy, gdy uruchomisz akcję samodzielnie.',
    },
    profile: {
      name: 'Informacje o profilu',
      text: 'Zadeklarowane na potrzeby odczytu aktywnego profilu lub projektu. Obecne ABI wtyczek nie ma funkcji, która z tego korzysta.',
    },
    variables: {
      name: 'Odczyt zmiennych',
      text: 'Zadeklarowane na potrzeby odczytu zmiennych należących do innych właścicieli. Obecne ABI wtyczek nie ma funkcji, która z tego korzysta.',
    },
    background: {
      name: 'Praca w tle',
      text: 'Zadeklarowane na potrzeby działania według harmonogramu. Obecne ABI wtyczek nie obsługuje pracy w tle.',
    },
    device: {
      name: 'Urządzenie',
      text: 'Zadeklarowane na potrzeby stanu urządzenia lub ikon przycisków. Obecne ABI wtyczek nie ma funkcji, która z tego korzysta.',
    },
    unknown: {
      name: 'Inne uprawnienie',
      text: 'Rodzaj uprawnienia, którego ta witryna nie zna. DeckPilot wyjaśni je, zanim je przyznasz.',
    },
    justificationLabel: 'Uzasadnienie podane przez wydawcę:',
    hosts: 'Hosty',
    methods: 'Metody',
    slots: 'Pola danych uwierzytelniających',
    quota: 'Limit',
    quotaValue: '{kb} KiB',
    userOrigins: 'Także adres serwera, który wpiszesz samodzielnie, po Twoim potwierdzeniu.',
    privateNetwork: 'Deklaruje adresy w sieci prywatnej.',
    reads: 'Odczytuje',
    interval: 'Minimalny odstęp',
    seconds: '{count} s',
  },

  docs: {
    title: 'Dokumentacja',
    metaTitle: 'Dokumentacja dla twórców wtyczek',
    description:
      'Dokumentacja dla twórców wtyczek do DeckPilota: SDK, manifest, akcje, uprawnienia, pamięć trwała, testowanie, pakowanie i zgłaszanie do rejestru.',
    lead: 'Wszystko, czego potrzeba, żeby napisać, przetestować i zgłosić wtyczkę do DeckPilota. Opisujemy to, co repozytorium DeckPilota implementuje dzisiaj.',
    groups: {
      start: 'Na początek',
      build: 'Budowanie',
      capabilities: 'Możliwości',
      ship: 'Testy i publikacja',
      reference: 'Informacje dodatkowe',
    },
    sidebarLabel: 'Strony dokumentacji',
    menu: 'Menu dokumentacji',
    onThisPage: 'Na tej stronie',
    previous: 'Poprzednia',
    next: 'Następna',
    pagination: 'Poprzednia i następna strona',
    statusTitle: 'DeckPilot 0.10.0 obsługuje API wtyczek 1.9',
    status:
      'Obsługa wtyczek istnieje od DeckPilota 0.7.0, a wtyczki są domyślnie włączone; bieżące wydanie 0.10.0 udostępnia API wtyczek 1.9, którego najnowsze dodatki są przeznaczone wyłącznie dla własnych wtyczek DeckPilota dołączonych do aplikacji. Podpisany rejestr działa i publikuje własne wtyczki DeckPilota; nie jest jeszcze otwarty dla innych wydawców. SDK ma wersję 0.x i nie jest opublikowane jako pakiet, więc jego API w Ruście może się jeszcze zmienić.',
    verified: 'Sprawdzono z kodem DeckPilota w commicie {commit}.',
    source: 'Źródłowa dokumentacja SDK w repozytorium DeckPilota',
  },

  developers: {
    title: 'Twórz wtyczki do DeckPilota',
    metaTitle: 'Portal dla twórców',
    description:
      'Pisz wtyczki do DeckPilota w języku Rust, kompiluj je do WebAssembly, testuj z atrapą hosta i zgłaszaj do podpisanego rejestru. Poradniki, SDK i przykłady.',
    lead: 'Wtyczka dodaje do DeckPilota nowe akcje. Piszesz ją w języku Rust z użyciem SDK wtyczek, kompilujesz do jednego modułu WebAssembly i opisujesz w manifeście. DeckPilot uruchamia ją w piaskownicy i daje jej tylko te uprawnienia, które przyznał użytkownik.',
    primary: 'Zacznij tutaj',
    secondary: 'Zobacz przykłady',
    facts: [
      {
        title: 'Jeden moduł WebAssembly',
        text: 'Wtyczka to pojedynczy plik plugin.wasm implementujący ABI gościa w wersji 1, a obok niego manifest.json, plik LICENSE i ikona. Bez WASI: wtyczka nie ma własnego dostępu do plików, sieci ani zegara.',
      },
      {
        title: 'Typowane SDK dla języka Rust',
        text: 'Crate deckpilot-plugin-sdk udostępnia akcje, parametry, wyniki oraz obiekt Context do obsługi HTTPS, danych uwierzytelniających, ustawień, zmiennych, pamięci trwałej i powiadomień.',
      },
      {
        title: 'Testy jak w każdym crate',
        text: 'Funkcje obsługi akcji uruchamiasz natywnie przez cargo test z atrapą hosta, a SDK sprawdza, czy kod i manifest są ze sobą zgodne.',
      },
    ],
    obtainTitle: 'Skąd wziąć SDK',
    obtain:
      'SDK nie jest opublikowane w crates.io i nie ma pakietu npm. Znajduje się w repozytorium DeckPilota: możesz budować wtyczkę wewnątrz kopii repozytorium albo dodać repozytorium jako zależność git przypiętą do konkretnego commita. SDK dla JavaScriptu w repozytorium to wyłącznie deklaracje typów. Wtyczek w JavaScripcie nie da się jeszcze zbudować ani uruchomić.',
    obtainLink: 'Pierwsze kroki',
    pathTitle: 'Od pomysłu do rejestru',
    pathLead: 'Droga, którą przechodzi każda wtyczka. Każdy krok prowadzi do swojego poradnika.',
    path: [
      { title: 'Napisz', text: 'Zadeklaruj akcje i parametry w kodzie oraz w manifeście.', doc: 'actions' },
      { title: 'Proś o mniej', text: 'Deklaruj tylko te uprawnienia, z których korzystasz, każde z uzasadnieniem.', doc: 'permissions' },
      { title: 'Przetestuj', text: 'Uruchom testy jednostkowe z atrapą hosta, a potem z prawdziwym agentem.', doc: 'testing' },
      { title: 'Spakuj', text: 'Zbuduj moduł dla wasm32-unknown-unknown i utwórz pakiet .dpplugin.', doc: 'packaging' },
      { title: 'Zgłoś', text: 'Otwórz w rejestrze pull request z rekordem swojej wersji.', doc: 'submission' },
    ],
    topicsTitle: 'Dokumentacja',
    topicsLead: 'Wszystkie poradniki, pogrupowane w kolejności, w jakiej będą Ci potrzebne.',
    examplesTitle: 'Przykłady',
    examplesLead: 'W repozytorium DeckPilota znajdują się trzy przykładowe wtyczki. W jego CI są budowane, pakowane i uruchamiane przez prawdziwego agenta. To przykłady, a nie opublikowane wtyczki.',
    examples: [
      { name: 'hello-world', text: 'Minimum: jedna czysta akcja z jednym parametrem. Bez uprawnień.' },
      { name: 'developer-tools', text: 'HTTPS do jednego hosta, dane uwierzytelniające używane przez uchwyt, ustawienia, pamięć trwała, zmienne, powiadomienia i typowane błędy.' },
      { name: 'hello-weather', text: 'Czysta akcja, jedno żądanie przez brokera i hak inicjalizacji.' },
    ],
    helpTitle: 'Pytania i problemy',
    help: 'Błąd w narzędziach rejestru zgłoś jako issue w repozytorium rejestru. Sprawy dotyczące SDK, piaskownicy lub aplikacji zgłaszaj w repozytorium DeckPilota. Luki w zabezpieczeniach zgłaszaj prywatnie.',
    helpIssues: 'Issue w repozytorium rejestru',
    helpSecurity: 'Polityka bezpieczeństwa',
  },

  community: {
    title: 'Społeczność',
    metaTitle: 'Społeczność i współpraca',
    description:
      'Jak zgłosić wtyczkę do rejestru DeckPilota, jak przebiega przegląd, jakie są zasady licencji, wersjonowania, aktualizacji i odwołań oraz jak zgłosić lukę.',
    lead: 'Rejestr jest prowadzony otwarcie, w jednym repozytorium w serwisie GitHub. Ta strona streszcza jego zasady. Wiążące są dokumenty, do których prowadzą odnośniki: jeśli ta strona i dokument się różnią, obowiązuje dokument.',
    statusTitle: 'Rejestr nie jest jeszcze otwarty dla innych wydawców',
    status:
      'Podpisany rejestr działa i publikuje własne wtyczki DeckPilota. Wtyczki innych wydawców nie są jeszcze publikowane. Zgłoszenie możesz już przygotować i sprawdzić; według zasad współpracy zostanie przejrzane i opublikowane, gdy rejestr zostanie otwarty.',
    toc: 'Na tej stronie',
    submit: {
      title: 'Zgłoś wtyczkę',
      lead: 'Twój kod pozostaje w Twoim własnym publicznym repozytorium w serwisie GitHub. Rejestr zapisuje jedynie, który tag i commit należy zbudować. Nigdy nie przechowuje Twojego kodu.',
      steps: [
        {
          title: 'Otaguj kod',
          text: 'Dodaj do repozytorium plik Cargo.lock, przypnij toolchain Rusta do dokładnej wersji w pliku rust-toolchain.toml i wypchnij tag. Nigdy nie przesuwaj ani nie usuwaj tagu, na który wskazuje rekord w rejestrze.',
        },
        {
          title: 'Zrób fork rejestru i dodaj swoje pliki',
          text: 'Rekord wydawcy (tylko przy pierwszej wtyczce), opis wtyczki, przepis budowania, kopię manifestu oraz rekord wersji wygenerowany przypiętymi narzędziami.',
        },
        {
          title: 'Uruchom kontrole',
          text: 'Jeśli możesz, lokalnie, skryptami tools/build-cli.sh i tools/validate.sh. Te same kontrole są uruchamiane dla Twojego pull requesta, bez dostępu do sekretów.',
        },
        {
          title: 'Otwórz pull request',
          text: 'Skorzystaj z szablonu i wypełnij listę kontrolną zgłoszenia wtyczki.',
        },
      ],
      namespace:
        'Identyfikator wtyczki ma postać <przestrzeń-nazw>.<nazwa>. Przestrzeń nazw staje się Twoja z chwilą scalenia rekordu wydawcy i jest powiązana z numerycznym identyfikatorem Twojego konta GitHub. Przestrzenią nazw nie mogą być identyfikatory zastrzeżone przez DeckPilota, takie jak deckpilot, knittingcode, core, system czy nazwy wbudowanych modułów.',
      guide: 'Poradnik zgłaszania krok po kroku',
      contributing: 'CONTRIBUTING.md',
    },
    review: {
      title: 'Jak przebiega przegląd',
      lead: 'Każde zgłoszenie przed scaleniem sprawdza właściciel repozytorium. Nic nie jest scalane automatycznie, a zielony wynik CI jest konieczny, ale nie oznacza akceptacji.',
      checks: [
        { title: 'Tożsamość wydawcy', text: 'Konto GitHub istnieje, jego numeryczny identyfikator zgadza się z rekordem, a przestrzeń nazw nie podszywa się pod inny projekt, firmę ani pod DeckPilota.' },
        { title: 'Kod źródłowy', text: 'Repozytorium i tag należą do autora, tag wskazuje na zapisany commit, a różnice względem poprzedniej wersji nie zawierają niczego niewyjaśnionego.' },
        { title: 'Uprawnienia', text: 'Każde uprawnienie ma uzasadnienie zgodne z tym, co robi wtyczka, a hosty sieciowe należą do usługi, którą wtyczka obsługuje. Nowe uprawnienie w aktualizacji jest sprawdzane równie starannie jak nowa wtyczka.' },
        { title: 'Budowanie', text: 'Dokładna wersja toolchaina, zablokowany workspace Cargo i zwykłe cargo build jednego pakietu. CI zbudowało pakiet od nowa i jest on identyczny z rekordem bajt po bajcie.' },
        { title: 'Licencja', text: 'Manifest deklaruje licencję w formacie SPDX, a pakiet zawiera plik LICENSE.' },
        { title: 'Opis', text: 'Nazwa, streszczenie, opis i kategorie są zgodne z prawdą i nie wprowadzają w błąd.' },
      ],
      publish:
        'Scalenie niczego nie publikuje. Wersję publikuje właściciel repozytorium, tworząc jej tag wydania. Współtwórcy nigdy nie otrzymują praw do publikacji automatycznie: ani dzięki scaleniu wtyczki, ani dzięki statusowi zweryfikowanego wydawcy, ani dzięki regularnemu wkładowi.',
      times: 'Nie ma gwarantowanych czasów odpowiedzi. Zgłoszenia dotyczące bezpieczeństwa są obsługiwane w pierwszej kolejności.',
      policy: 'Zasady przeglądu',
    },
    security: {
      title: 'Wymagania bezpieczeństwa',
      lead: 'Co musi spełniać zgłoszenie, poza przejściem automatycznych kontroli.',
      items: [
        'Deklaruj tylko te uprawnienia, z których wtyczka korzysta. Każde wymaga od użytkownika decyzji o zgodzie i podlega przeglądowi.',
        'Hosty sieciowe muszą należeć do usługi, dla której powstała wtyczka. Hosty do otwierania linków muszą być stronami, które akcje wtyczki otwierają użytkownikowi, nigdy szerokim wzorcem.',
        'Pola danych uwierzytelniających muszą odpowiadać hostom sieciowym wtyczki.',
        'Budowanie musi być hermetyczne: bez dostępu do sieci poza zablokowanymi zależnościami Cargo i bez pobierania czegokolwiek w czasie budowania.',
        'Bez zaciemniania kodu, wygenerowanych blobów, dołączonych plików binarnych i skryptów budowania, które cokolwiek pobierają lub uruchamiają.',
        'Nigdy nie dodawaj do repozytorium klucza prywatnego, pliku klucza, pakietu ani wygenerowanych plików. Kontrola higieny repozytorium je odrzuca.',
      ],
      guide: 'Zasady bezpieczeństwa dla autorów wtyczek',
    },
    licenses: {
      title: 'Licencje',
      items: [
        'Wybierz licencję dla swojej wtyczki, zadeklaruj ją w pliku manifest.json jako wyrażenie SPDX i dołącz plik LICENSE do katalogu wtyczki.',
        'Wtyczka zachowuje własną licencję. Umieszczenie jej w rejestrze nie zmienia jej licencji, a licencja Apache-2.0 rejestru nie obejmuje kodu ani pakietów wtyczek.',
        'Wkład we własne pliki rejestru (dokumentację, narzędzia) jest objęty jego licencją Apache-2.0.',
        'Podpis DCO (git commit -s) jest mile widziany, ale nieobowiązkowy. Nie ma umowy CLA.',
      ],
      note: 'Rejestr nie publikuje listy akceptowanych licencji. Licencja jest sprawdzana podczas przeglądu.',
      link: 'LICENSE',
    },
    versioning: {
      title: 'Wersjonowanie i aktualizacje',
      items: [
        'Wersje są zgodne z wersjonowaniem semantycznym, a nowa wersja musi być wyższa od każdej już zapisanej.',
        'Zapisane wersje są niezmienne: nigdy ich nie edytuj ani nie usuwaj. Żeby coś zmienić, opublikuj nową wersję.',
        'Aby zaktualizować wtyczkę, dodaj nowy rekord wersji, zastąp kopię manifestu nowym manifestem i w razie potrzeby zmień opis wtyczki lub przepis budowania.',
        'Jeśli aktualizacja dodaje uprawnienia, wyjaśnij powód w pull requeście. Użytkownicy są proszeni o zatwierdzenie dodanych uprawnień, zanim aktualizacja zostanie zastosowana.',
        'Tag wydania opublikowanej wersji ma zawsze postać <id>--v<wersja> i tworzy go właściciel repozytorium.',
      ],
      link: 'Proces publikacji',
    },
    revocation: {
      title: 'Odwołania',
      lead: 'Opublikowaną wtyczkę lub wersję właściciel repozytorium może odwołać w sprawdzonym pull requeście, po którym publikowany jest nowy podpisany indeks. Aplikacje stosują odwołanie przy najbliższym udanym odświeżeniu rejestru i od tej chwili egzekwują je także offline.',
      yankTitle: 'Wycofanie wersji (yank)',
      yank: 'Wersja nie jest już oferowana do instalacji ani aktualizacji. Zainstalowane kopie działają dalej, z odpowiednią informacją.',
      disableTitle: 'Wyłączenie (disable)',
      disable: 'DeckPilot zatrzymuje zainstalowaną wtyczkę i odmawia jej ponownego wczytania. Stosowane wobec wtyczek złośliwych lub niebezpiecznych.',
      cumulative: 'Odwołania się kumulują: nowy indeks musi zachować każde opublikowane odwołanie w co najmniej tak samo silnej postaci. Zniesienie odwołania wymaga ceremonii kluczy głównych przeprowadzanej offline.',
      link: 'Model zaufania',
    },
    reporting: {
      title: 'Zgłaszanie luk w zabezpieczeniach',
      lead: 'Złośliwą wtyczkę, lukę w rejestrze lub w procesie publikacji albo ujawniony klucz zgłoś prywatnie, korzystając z prywatnego zgłaszania luk w serwisie GitHub, w repozytorium rejestru. Nie ma osobnego adresu e-mail ani innego kanału.',
      dont: 'Nie otwieraj publicznego issue ani pull requesta w sprawie podejrzenia złośliwej wtyczki lub luki, nie publikuj szczegółów exploita i nie testuj niczego na instalacjach innych użytkowników.',
      scope: 'Luki w aplikacji lub agencie DeckPilota, w piaskownicy i procesie hosta wtyczek, w SDK lub w kodzie weryfikacji po stronie klienta zgłaszaj w repozytorium DeckPilota. Problem z wtyczką niezwiązany z bezpieczeństwem zgłoś formularzem issue „Plugin report”.',
      after: 'Nie ma gwarantowanych czasów odpowiedzi. Zgłoszenia są obsługiwane przed wszystkimi innymi pracami. Osoby zgłaszające są wymieniane w komunikacie bezpieczeństwa, chyba że sobie tego nie życzą.',
      caveat: 'Rejestr nie przeszedł niezależnego audytu bezpieczeństwa.',
      primary: 'Zgłoś lukę',
      policy: 'SECURITY.md',
      appPolicy: 'Zgłoś lukę w DeckPilocie',
    },
    conduct: {
      title: 'Kodeks postępowania',
      text: 'Biorąc udział w projekcie, akceptujesz Kodeks postępowania.',
      link: 'CODE_OF_CONDUCT.md',
    },
    other: {
      title: 'Inne sposoby na współpracę',
      text: 'Poprawki literówek, doprecyzowania i naprawy niedziałających linków w dokumentacji są mile widziane jako zwykłe pull requesty. Problem z narzędziami lub workflowem zgłoś jako issue z raportem o błędzie, a nie jako pull request: tymi ścieżkami opiekuje się właściciel repozytorium.',
      issues: 'Otwórz issue',
    },
  },

  notFound: {
    title: 'Nie znaleziono strony',
    description: 'Strona, której szukasz, nie istnieje w DeckPilot Plugin Marketplace.',
    heading: 'Ta strona nie istnieje',
    text: 'Adres może zawierać literówkę albo strona została przeniesiona. Nie ma to wpływu na pliki rejestru, które odczytuje DeckPilot.',
    home: 'Wróć na stronę główną',
    plugins: 'Przeglądaj wtyczki',
  },

  root: {
    title: 'DeckPilot Plugin Marketplace',
    description: 'Wybierz język, aby przejść do DeckPilot Plugin Marketplace.',
    heading: 'Wybierz język',
  },
};
