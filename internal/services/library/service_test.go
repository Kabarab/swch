package library

import (
	"fmt"
	"path/filepath"
	"sync/atomic"
	"testing"

	"swch/internal/bus"
	"swch/internal/interfaces"
	"swch/internal/models"
)

// granularProvider реализует LocalLibraryScanner + RemoteLibraryProvider
// и считает обращения к источникам.
type granularProvider struct {
	localCalls  atomic.Int32
	remoteCalls atomic.Int32
	remoteErr   error
}

func (g *granularProvider) GetName() string                                 { return "Granular" }
func (g *granularProvider) GetAccountsProvider() ([]models.Account, error)  { return nil, nil }
func (g *granularProvider) SwitchAccount(name string) error                 { return nil }
func (g *granularProvider) LaunchGame(gameID, exePath string) error         { return nil }
func (g *granularProvider) GetGamesProvider() ([]models.LibraryGame, error) { return nil, nil }

func (g *granularProvider) ScanInstalledGames() ([]models.LibraryGame, error) {
	g.localCalls.Add(1)
	return []models.LibraryGame{
		{ID: "game_a", Name: "Game A", Platform: "Granular", ExePath: "/games/a", IsInstalled: true},
	}, nil
}

func (g *granularProvider) GetRemoteLibrary() ([]models.LibraryGame, error) {
	g.remoteCalls.Add(1)
	if g.remoteErr != nil {
		return nil, g.remoteErr
	}
	return []models.LibraryGame{
		{ID: "game_a", Name: "Game A", Platform: "Granular", IsInstalled: false,
			AvailableOnAccounts: []models.AccountStat{{AccountID: "acc1", Username: "user1"}}},
		{ID: "game_b", Name: "Game B", Platform: "Granular", IsInstalled: false},
	}, nil
}

// legacyGamesProvider — только базовый PlatformProvider (как Battle.net и др.)
type legacyGamesProvider struct {
	calls atomic.Int32
}

func (l *legacyGamesProvider) GetName() string                                { return "Legacy" }
func (l *legacyGamesProvider) GetAccountsProvider() ([]models.Account, error) { return nil, nil }
func (l *legacyGamesProvider) SwitchAccount(name string) error                { return nil }
func (l *legacyGamesProvider) LaunchGame(gameID, exePath string) error        { return nil }

func (l *legacyGamesProvider) GetGamesProvider() ([]models.LibraryGame, error) {
	l.calls.Add(1)
	return []models.LibraryGame{
		{ID: "game_c", Name: "Game C", Platform: "Legacy", IsInstalled: true},
	}, nil
}

type countingEmitter struct {
	events []string
}

func (c *countingEmitter) Emit(event string, data ...interface{}) {
	c.events = append(c.events, event)
}

func (c *countingEmitter) count(event string) int {
	n := 0
	for _, e := range c.events {
		if e == event {
			n++
		}
	}
	return n
}

func newTestProviders() (map[string]interfaces.PlatformProvider, *granularProvider, *legacyGamesProvider) {
	g := &granularProvider{}
	l := &legacyGamesProvider{}
	return map[string]interfaces.PlatformProvider{"Granular": g, "Legacy": l}, g, l
}

func TestRefresh_MergesAllSources(t *testing.T) {
	providers, _, _ := newTestProviders()
	emitter := &countingEmitter{}
	svc := NewService(providers, emitter, filepath.Join(t.TempDir(), "cache.json"))

	games := svc.Refresh()

	if len(games) != 3 {
		t.Fatalf("expected 3 games (A merged, B, C), got %d: %+v", len(games), games)
	}

	byID := make(map[string]models.LibraryGame)
	for _, g := range games {
		byID[g.ID] = g
	}
	a := byID["game_a"]
	if !a.IsInstalled || a.ExePath != "/games/a" {
		t.Errorf("game_a: installed local version must win, got %+v", a)
	}
	if len(a.AvailableOnAccounts) != 1 {
		t.Errorf("game_a: owners from remote must be merged in, got %+v", a.AvailableOnAccounts)
	}

	// 2 источника granular + 1 legacy = 3 события прогресса + 1 library-updated
	if n := emitter.count(bus.EventLibraryRefreshProgress); n != 3 {
		t.Errorf("expected 3 progress events, got %d", n)
	}
	if n := emitter.count(bus.EventLibraryUpdated); n != 1 {
		t.Errorf("expected 1 library-updated event, got %d", n)
	}
}

func TestGet_UsesCacheAfterFirstRefresh(t *testing.T) {
	providers, g, l := newTestProviders()
	svc := NewService(providers, nil, filepath.Join(t.TempDir(), "cache.json"))

	first := svc.Get() // пустой кэш -> синхронный Refresh
	if len(first) == 0 {
		t.Fatal("first Get returned no games")
	}
	if g.localCalls.Load() != 1 || l.calls.Load() != 1 {
		t.Fatalf("first Get must scan providers once")
	}

	second := svc.Get() // из кэша, без обращений к провайдерам
	if len(second) != len(first) {
		t.Errorf("cached result differs: %d vs %d", len(second), len(first))
	}
	if g.localCalls.Load() != 1 || l.calls.Load() != 1 {
		t.Errorf("second Get must not rescan providers (local=%d, legacy=%d)",
			g.localCalls.Load(), l.calls.Load())
	}
}

func TestRefresh_Debounce(t *testing.T) {
	providers, g, _ := newTestProviders()
	svc := NewService(providers, nil, "")

	svc.Refresh()
	svc.Refresh() // сразу повторно — должен вернуть кэш без рескана

	if n := g.localCalls.Load(); n != 1 {
		t.Errorf("debounce failed: provider scanned %d times", n)
	}
}

func TestRefresh_RemoteErrorKeepsLocalResults(t *testing.T) {
	g := &granularProvider{remoteErr: fmt.Errorf("api down")}
	providers := map[string]interfaces.PlatformProvider{"Granular": g}
	emitter := &countingEmitter{}
	svc := NewService(providers, emitter, "")

	games := svc.Refresh()

	// Ошибка API не должна терять локально найденные игры
	if len(games) != 1 || games[0].ID != "game_a" || !games[0].IsInstalled {
		t.Errorf("expected local game_a to survive remote failure, got %+v", games)
	}
}

func TestCachePersistence_AcrossServiceRestarts(t *testing.T) {
	cachePath := filepath.Join(t.TempDir(), "cache.json")
	providers, _, _ := newTestProviders()

	svc1 := NewService(providers, nil, cachePath)
	got := svc1.Refresh()

	// Новый сервис с тем же кэшем и БЕЗ провайдеров: данные должны
	// подняться с диска без единого скана
	svc2 := NewService(map[string]interfaces.PlatformProvider{}, nil, cachePath)
	restored := svc2.Get()

	if len(restored) != len(got) {
		t.Errorf("cache not restored from disk: %d vs %d games", len(restored), len(got))
	}
}
