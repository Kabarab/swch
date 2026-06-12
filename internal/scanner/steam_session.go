package scanner

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"swch/internal/sys"
)

// --- Steam: управление процессом и снапшоты сессий (TcNo-style) ---
//
// Сессия Steam состоит из:
//   config/loginusers.vdf  — залогиненные аккаунты и их токены
//   config/config.vdf      — ConnectCache и настройки клиента
//   ssfn*                  — sentry-файлы (легаси-2FA на старых установках)
//   registry.vdf           — только macOS (аналог реестра Windows)
// Активный аккаунт дополнительно задается через sys.SetSteamUser:
// реестр HKCU\Software\Valve\Steam на Windows, registry.vdf на macOS.
//
// Снапшоты хранятся в UserConfigDir/swch/data/steam/<имя_аккаунта>/
// механизмом FileSwapConfig (см. fileswap.go).

const steamSwapID = "steam"

// sessionSwapConfig собирает актуальный список файлов сессии.
// Вызывается каждый раз заново: имена ssfn-файлов содержат случайный суффикс.
func (s *SteamScanner) sessionSwapConfig() (*FileSwapConfig, error) {
	if s.Path == "" {
		return nil, fmt.Errorf("steam path not found")
	}

	files := []string{
		filepath.Join(s.Path, "config", "loginusers.vdf"),
		filepath.Join(s.Path, "config", "config.vdf"),
	}

	// macOS: registry.vdf лежит в корне или в config/ в зависимости от версии.
	// Basename у обоих одинаковый, поэтому в снапшот попадает первый найденный.
	for _, p := range []string{
		filepath.Join(s.Path, "registry.vdf"),
		filepath.Join(s.Path, "config", "registry.vdf"),
	} {
		if _, err := os.Stat(p); err == nil {
			files = append(files, p)
			break
		}
	}

	// Sentry-файлы в корне Steam
	if entries, err := os.ReadDir(s.Path); err == nil {
		for _, e := range entries {
			if !e.IsDir() && strings.HasPrefix(e.Name(), "ssfn") {
				files = append(files, filepath.Join(s.Path, e.Name()))
			}
		}
	}

	return &FileSwapConfig{
		ID:                  steamSwapID,
		ProcessNames:        []string{"steam.exe", "steamwebhelper.exe", "GameOverlayUI.exe"},
		Files:               files,
		StoragePathOverride: s.StorageOverride,
	}, nil
}

// KillLauncher implements interfaces.ProcessController.
func (s *SteamScanner) KillLauncher() error {
	sys.KillSteam()
	return nil
}

// StartLauncher implements interfaces.ProcessController.
func (s *SteamScanner) StartLauncher() error {
	sys.StartGame("steam://open/main")
	return nil
}

// ActivateAccount implements interfaces.AccountActivator:
// прописывает аккаунт в реестре/registry.vdf и loginusers.vdf, не трогая процесс.
func (s *SteamScanner) ActivateAccount(accountName string) error {
	if accountName == "" || accountName == "UNKNOWN" {
		return fmt.Errorf("login not found")
	}
	if err := sys.SetSteamUser(accountName); err != nil {
		return fmt.Errorf("failed to set autologin user: %w", err)
	}
	if err := s.SetUserActive(accountName); err != nil {
		return fmt.Errorf("error updating VDF: %w", err)
	}
	return nil
}

// SaveSession implements interfaces.SessionSnapshotter.
// Лаунчер останавливать не требуется: файлы только читаются.
func (s *SteamScanner) SaveSession(accountName string) error {
	cfg, err := s.sessionSwapConfig()
	if err != nil {
		return err
	}
	if err := cfg.SaveState(accountName); err != nil {
		return fmt.Errorf("failed to save steam session: %w", err)
	}
	fmt.Printf("[Steam] Session snapshot saved for '%s'\n", accountName)
	return nil
}

// RestoreSession implements interfaces.SessionSnapshotter.
// Вызывать при закрытом Steam — switcher.Service делает kill до этого шага.
func (s *SteamScanner) RestoreSession(accountName string) error {
	cfg, err := s.sessionSwapConfig()
	if err != nil {
		return err
	}

	storage, err := cfg.GetStoragePath()
	if err != nil {
		return err
	}
	snapDir := filepath.Join(storage, accountName)
	if _, err := os.Stat(snapDir); os.IsNotExist(err) {
		return fmt.Errorf("session snapshot not found for '%s' (save it first)", accountName)
	}

	// ssfn-файлы из снапшота, которых сейчас нет в корне Steam (например, Steam
	// удалил их после logout), тоже должны вернуться на место.
	if entries, err := os.ReadDir(filepath.Join(snapDir, "FILES")); err == nil {
		for _, e := range entries {
			if strings.HasPrefix(e.Name(), "ssfn") && !containsBaseName(cfg.Files, e.Name()) {
				cfg.Files = append(cfg.Files, filepath.Join(s.Path, e.Name()))
			}
		}
	}

	if err := cfg.LoadState(accountName); err != nil {
		return fmt.Errorf("failed to restore steam session: %w", err)
	}
	fmt.Printf("[Steam] Session snapshot restored for '%s'\n", accountName)
	return nil
}

// ListSessions implements interfaces.SessionSnapshotter.
func (s *SteamScanner) ListSessions() ([]string, error) {
	cfg, err := s.sessionSwapConfig()
	if err != nil {
		return nil, err
	}
	storage, err := cfg.GetStoragePath()
	if err != nil {
		return nil, err
	}

	entries, err := os.ReadDir(storage)
	if err != nil {
		if os.IsNotExist(err) {
			return []string{}, nil // снапшотов еще не делали
		}
		return nil, err
	}

	var names []string
	for _, e := range entries {
		if e.IsDir() {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)
	return names, nil
}

// DeleteSession implements interfaces.SessionSnapshotter.
func (s *SteamScanner) DeleteSession(accountName string) error {
	cfg, err := s.sessionSwapConfig()
	if err != nil {
		return err
	}
	storage, err := cfg.GetStoragePath()
	if err != nil {
		return err
	}
	target := filepath.Join(storage, accountName)
	if _, err := os.Stat(target); os.IsNotExist(err) {
		return fmt.Errorf("session snapshot not found for '%s'", accountName)
	}
	return os.RemoveAll(target)
}

func containsBaseName(paths []string, base string) bool {
	for _, p := range paths {
		if filepath.Base(p) == base {
			return true
		}
	}
	return false
}
