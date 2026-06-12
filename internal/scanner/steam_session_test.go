package scanner

import (
	"os"
	"path/filepath"
	"testing"
)

// newMockSteam создает временную структуру папок, имитирующую установленный
// Steam (PRD 4.2: Test Fixtures Injection), и SteamScanner поверх нее.
func newMockSteam(t *testing.T) *SteamScanner {
	t.Helper()
	tempDir := t.TempDir()

	steamPath := filepath.Join(tempDir, "Steam")
	configDir := filepath.Join(steamPath, "config")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatal(err)
	}

	fixtures := map[string]string{
		filepath.Join(configDir, "loginusers.vdf"): `"users"
{
	"76561197960265729"
	{
		"AccountName"		"player1"
		"PersonaName"		"Player One"
		"MostRecent"		"1"
	}
}`,
		filepath.Join(configDir, "config.vdf"):     `"InstallConfigStore" { "Software" { } }`,
		filepath.Join(steamPath, "registry.vdf"):   `"Registry" { "HKCU" { "Software" { "Valve" { "Steam" { "AutoLoginUser" "player1" } } } } }`,
		filepath.Join(steamPath, "ssfn1234567890"): "sentry-binary-data",
	}
	for path, content := range fixtures {
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
	}

	return &SteamScanner{
		Path:            steamPath,
		StorageOverride: filepath.Join(tempDir, "storage"),
	}
}

func readFileOrFail(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("cannot read %s: %v", path, err)
	}
	return string(data)
}

func TestSteamSession_SaveAndRestore(t *testing.T) {
	s := newMockSteam(t)
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	ssfnPath := filepath.Join(s.Path, "ssfn1234567890")
	originalLoginUsers := readFileOrFail(t, loginUsersPath)

	// 1. Снапшот текущей сессии
	if err := s.SaveSession("player1"); err != nil {
		t.Fatalf("SaveSession failed: %v", err)
	}

	backup := filepath.Join(s.StorageOverride, "player1", "FILES", "loginusers.vdf")
	if _, err := os.Stat(backup); os.IsNotExist(err) {
		t.Fatalf("backup not created at %s", backup)
	}

	// 2. Имитируем смену состояния: другой пользователь + Steam удалил sentry
	if err := os.WriteFile(loginUsersPath, []byte(`"users" { }`), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(ssfnPath); err != nil {
		t.Fatal(err)
	}

	// 3. Восстановление снапшота
	if err := s.RestoreSession("player1"); err != nil {
		t.Fatalf("RestoreSession failed: %v", err)
	}

	if got := readFileOrFail(t, loginUsersPath); got != originalLoginUsers {
		t.Errorf("loginusers.vdf not restored:\n got:  %s\n want: %s", got, originalLoginUsers)
	}
	// Удаленный sentry-файл должен вернуться из снапшота
	if got := readFileOrFail(t, ssfnPath); got != "sentry-binary-data" {
		t.Errorf("ssfn file not restored, got: %q", got)
	}
}

func TestSteamSession_ListAndDelete(t *testing.T) {
	s := newMockSteam(t)

	// Пустое хранилище — пустой список, без ошибки
	names, err := s.ListSessions()
	if err != nil {
		t.Fatalf("ListSessions on empty storage failed: %v", err)
	}
	if len(names) != 0 {
		t.Errorf("expected empty list, got %v", names)
	}

	if err := s.SaveSession("acc_b"); err != nil {
		t.Fatal(err)
	}
	if err := s.SaveSession("acc_a"); err != nil {
		t.Fatal(err)
	}

	names, err = s.ListSessions()
	if err != nil {
		t.Fatalf("ListSessions failed: %v", err)
	}
	if len(names) != 2 || names[0] != "acc_a" || names[1] != "acc_b" {
		t.Errorf("expected sorted [acc_a acc_b], got %v", names)
	}

	if err := s.DeleteSession("acc_a"); err != nil {
		t.Fatalf("DeleteSession failed: %v", err)
	}
	names, _ = s.ListSessions()
	if len(names) != 1 || names[0] != "acc_b" {
		t.Errorf("expected [acc_b] after delete, got %v", names)
	}
}

func TestSteamSession_RestoreMissingSnapshot(t *testing.T) {
	s := newMockSteam(t)
	if err := s.RestoreSession("ghost"); err == nil {
		t.Fatal("expected error when restoring non-existent snapshot")
	}
}

func TestSteamSession_DeleteMissingSnapshot(t *testing.T) {
	s := newMockSteam(t)
	if err := s.DeleteSession("ghost"); err == nil {
		t.Fatal("expected error when deleting non-existent snapshot")
	}
}

func TestSteamSession_NoSteamPath(t *testing.T) {
	s := &SteamScanner{Path: ""}
	if err := s.SaveSession("player1"); err == nil {
		t.Fatal("expected error when steam path is unknown")
	}
}
