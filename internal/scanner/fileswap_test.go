package scanner

import (
	"os"
	"path/filepath"
	"testing"
)

func TestFileSwapConfig_SaveAndLoad(t *testing.T) {
	// 1. Setup temporary environment
	tempDir, err := os.MkdirTemp("", "swch_test")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(tempDir) // Clean up after test

	// Mock "Original" Data
	appDataDir := filepath.Join(tempDir, "AppData")
	configDir := filepath.Join(appDataDir, "Config")
	err = os.MkdirAll(configDir, 0755)
	if err != nil {
		t.Fatal(err)
	}

	configFile := filepath.Join(configDir, "settings.json")
	originalContent := []byte(`{"user":"player1"}`)
	if err := os.WriteFile(configFile, originalContent, 0644); err != nil {
		t.Fatal(err)
	}

	// Mock Storage Directory
	storageDir := filepath.Join(tempDir, "SwchData")
	err = os.MkdirAll(storageDir, 0755)
	if err != nil {
		t.Fatal(err)
	}

	// 2. Configure the Swap
	config := &FileSwapConfig{
		ID:                  "test_provider",
		StoragePathOverride: storageDir,
		Directories:         []string{configDir},
		// Files: []string{}, // directory covers included files
	}

	// 3. Test SaveState (Backup)
	accountName := "AccountOne"
	err = config.SaveState(accountName)
	if err != nil {
		t.Fatalf("SaveState failed: %v", err)
	}

	// Verify backup exists
	backupFile := filepath.Join(storageDir, accountName, "DIRS", "Config", "settings.json")
	if _, err := os.Stat(backupFile); os.IsNotExist(err) {
		t.Errorf("Backup file not created at %s", backupFile)
	}

	// 4. Modify "Original" to simulate state change
	newContent := []byte(`{"user":"player2"}`)
	if err := os.WriteFile(configFile, newContent, 0644); err != nil {
		t.Fatal(err)
	}

	// 5. Test LoadState (Restore)
	err = config.LoadState(accountName)
	if err != nil {
		t.Fatalf("LoadState failed: %v", err)
	}

	// Verify original content restored
	restoredContent, err := os.ReadFile(configFile)
	if err != nil {
		t.Fatal(err)
	}
	if string(restoredContent) != string(originalContent) {
		t.Errorf("Restore failed. Got %s, want %s", string(restoredContent), string(originalContent))
	}
}
