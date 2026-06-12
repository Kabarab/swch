package scanner

import (
	"os"
	"path/filepath"
	"testing"
)

// newMockSteamLibrary создает фейковую структуру установленного Steam
// с библиотекой игр и аккаунтом (PRD 4.2: Test Fixtures Injection):
//
//	Steam/
//	  config/loginusers.vdf            — аккаунт player1 (id64 -> id3=1)
//	  userdata/1/config/localconfig.vdf — владение играми 730 и 570
//	  steamapps/appmanifest_730.acf     — установленная CS2
//	  steamapps/common/csgo/            — папка игры
func newMockSteamLibrary(t *testing.T) *SteamScanner {
	t.Helper()
	tempDir := t.TempDir()
	steamPath := filepath.Join(tempDir, "Steam")

	dirs := []string{
		filepath.Join(steamPath, "config"),
		filepath.Join(steamPath, "userdata", "1", "config"),
		filepath.Join(steamPath, "steamapps", "common", "csgo"),
	}
	for _, d := range dirs {
		if err := os.MkdirAll(d, 0755); err != nil {
			t.Fatal(err)
		}
	}

	fixtures := map[string]string{
		filepath.Join(steamPath, "config", "loginusers.vdf"): `"users"
{
	"76561197960265729"
	{
		"AccountName"		"player1"
		"PersonaName"		"Player One"
		"MostRecent"		"1"
	}
}`,
		filepath.Join(steamPath, "steamapps", "appmanifest_730.acf"): `"AppState"
{
	"appid"		"730"
	"name"		"Counter-Strike 2"
	"installdir"		"csgo"
}`,
		filepath.Join(steamPath, "userdata", "1", "config", "localconfig.vdf"): `"UserLocalConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"apps"
				{
					"730"
					{
						"playtime"		"100"
					}
					"570"
					{
						"name"		"Dota 2"
					}
				}
			}
		}
	}
}`,
	}
	for path, content := range fixtures {
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
	}

	return &SteamScanner{Path: steamPath}
}

func TestSteamScanInstalledGames(t *testing.T) {
	s := newMockSteamLibrary(t)

	games, err := s.ScanInstalledGames()
	if err != nil {
		t.Fatalf("ScanInstalledGames failed: %v", err)
	}

	if len(games) != 1 {
		t.Fatalf("expected 1 installed game, got %d: %+v", len(games), games)
	}

	g := games[0]
	if g.ID != "730" || g.Name != "Counter-Strike 2" || g.Platform != "Steam" {
		t.Errorf("unexpected game data: %+v", g)
	}
	if !g.IsInstalled {
		t.Error("manifest-scanned game must be IsInstalled=true")
	}
	wantPath := filepath.Join(s.Path, "steamapps", "common", "csgo")
	if g.ExePath != wantPath {
		t.Errorf("ExePath = %q, want %q", g.ExePath, wantPath)
	}
	// Владелец определяется по localconfig.vdf аккаунта
	if len(g.AvailableOnAccounts) != 1 || g.AvailableOnAccounts[0].Username != "player1" {
		t.Errorf("owner attribution failed: %+v", g.AvailableOnAccounts)
	}
}

func TestSteamScanInstalledGames_NoPath(t *testing.T) {
	s := &SteamScanner{Path: ""}
	if _, err := s.ScanInstalledGames(); err == nil {
		t.Fatal("expected error when steam path is unknown")
	}
}

func TestSteamOwnedGamesFromConfigs(t *testing.T) {
	s := newMockSteamLibrary(t)
	accounts := s.GetAccounts()
	if len(accounts) != 1 {
		t.Fatalf("expected 1 account from mock, got %d", len(accounts))
	}

	owned := s.ownedGamesFromConfigs(accounts[0])

	if len(owned) != 2 {
		t.Fatalf("expected 2 owned games, got %d: %v", len(owned), owned)
	}
	if name, found := owned["730"]; !found || name != "" {
		t.Errorf(`owned["730"] = (%q, %v), want ("", true) — имя в конфиге отсутствует`, name, found)
	}
	if name := owned["570"]; name != "Dota 2" {
		t.Errorf(`owned["570"] = %q, want "Dota 2"`, name)
	}
}
