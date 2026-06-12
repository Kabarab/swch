package models

import "testing"

func TestMergeLibraries_InstalledWinsAndOwnersUnion(t *testing.T) {
	installed := []LibraryGame{
		{
			ID: "730", Name: "Counter-Strike 2", Platform: "Steam",
			ExePath: "C:\\Games\\CS2", IsInstalled: true,
			AvailableOnAccounts: []AccountStat{{AccountID: "1", Username: "player1"}},
		},
	}
	remote := []LibraryGame{
		{
			ID: "730", Name: "Counter-Strike 2", Platform: "Steam",
			IsInstalled: false,
			AvailableOnAccounts: []AccountStat{
				{AccountID: "1", Username: "player1"}, // дубликат — не должен задвоиться
				{AccountID: "2", Username: "player2"},
			},
		},
		{ID: "570", Name: "Dota 2", Platform: "Steam", IsInstalled: false},
	}

	merged := MergeLibraries(installed, remote)

	if len(merged) != 2 {
		t.Fatalf("expected 2 games, got %d", len(merged))
	}
	cs2 := merged[0]
	if !cs2.IsInstalled || cs2.ExePath != "C:\\Games\\CS2" {
		t.Errorf("installed version must win: IsInstalled=%v, ExePath=%q", cs2.IsInstalled, cs2.ExePath)
	}
	if len(cs2.AvailableOnAccounts) != 2 {
		t.Errorf("owners must be unioned without duplicates, got %v", cs2.AvailableOnAccounts)
	}
}

func TestMergeLibraries_OrderIndependence(t *testing.T) {
	installed := []LibraryGame{{ID: "1", Platform: "Steam", IsInstalled: true, ExePath: "/games/a"}}
	remote := []LibraryGame{{ID: "1", Platform: "Steam", IsInstalled: false}}

	// remote первым, installed вторым — флаг установки все равно должен победить
	merged := MergeLibraries(remote, installed)
	if !merged[0].IsInstalled || merged[0].ExePath != "/games/a" {
		t.Errorf("installed flags lost when installed list comes second: %+v", merged[0])
	}
}

func TestMergeLibraries_PlaceholderNameReplaced(t *testing.T) {
	a := []LibraryGame{{ID: "271590", Name: "Steam App 271590", Platform: "Steam"}}
	b := []LibraryGame{{ID: "271590", Name: "Grand Theft Auto V", Platform: "Steam"}}

	merged := MergeLibraries(a, b)
	if merged[0].Name != "Grand Theft Auto V" {
		t.Errorf("placeholder name must be replaced, got %q", merged[0].Name)
	}

	// Обратный порядок: настоящее имя не должно затираться плейсхолдером
	merged = MergeLibraries(b, a)
	if merged[0].Name != "Grand Theft Auto V" {
		t.Errorf("real name must not be replaced by placeholder, got %q", merged[0].Name)
	}
}

func TestMergeLibraries_DifferentPlatformsNotMerged(t *testing.T) {
	a := []LibraryGame{{ID: "gta5", Platform: "Steam"}}
	b := []LibraryGame{{ID: "gta5", Platform: "Epic"}}

	merged := MergeLibraries(a, b)
	if len(merged) != 2 {
		t.Fatalf("same ID on different platforms must stay separate, got %d entries", len(merged))
	}
}

func TestMergeLibraries_SourcesNotMutated(t *testing.T) {
	src := []LibraryGame{{ID: "1", Platform: "Steam",
		AvailableOnAccounts: []AccountStat{{AccountID: "1"}}}}
	more := []LibraryGame{{ID: "1", Platform: "Steam",
		AvailableOnAccounts: []AccountStat{{AccountID: "2"}}}}

	MergeLibraries(src, more)
	if len(src[0].AvailableOnAccounts) != 1 {
		t.Errorf("source slice was mutated by merge: %v", src[0].AvailableOnAccounts)
	}
}
