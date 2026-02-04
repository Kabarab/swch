package scanner

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"time"
)

// FileSwapConfig defines the paths required for a generic file-swapping account switcher.
type FileSwapConfig struct {
	ID           string   // Unique ID for the provider (e.g., "battlenet", "ubisoft")
	ProcessNames []string // Names of processes to kill before swapping (e.g., "Battle.net.exe")

	// Paths to backup/restore.
	// These are absolute paths on the system (e.g., %APPDATA%/Battle.net)
	Directories []string
	Files       []string

	// Optional: Override for testing or custom storage locations
	StoragePathOverride string
}

// GetStoragePath returns the base directory where swch stores account data for this provider.
// e.g. %APPDATA%/swch/data/battlenet/
func (c *FileSwapConfig) GetStoragePath() (string, error) {
	if c.StoragePathOverride != "" {
		return c.StoragePathOverride, nil
	}
	configDir, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	// swch storage usually in UserConfigDir or where app stores data.
	// We'll use os.UserConfigDir()/swch/data/<ID>
	return filepath.Join(configDir, "swch", "data", c.ID), nil
}

// KillProcesses terminates the launcher processes.
func (c *FileSwapConfig) KillProcesses() error {
	if runtime.GOOS != "windows" {
		return nil // Only support Windows process killing for these launchers
	}
	for _, proc := range c.ProcessNames {
		cmd := exec.Command("taskkill", "/F", "/IM", proc)
		cmd.Run() // Ignore errors (process might not be running)
	}
	// Give it a moment to release file locks
	time.Sleep(1 * time.Second)
	return nil
}

// SaveState backups the current state files to a named account folder.
func (c *FileSwapConfig) SaveState(accountName string) error {
	storageBase, err := c.GetStoragePath()
	if err != nil {
		return err
	}
	targetDir := filepath.Join(storageBase, accountName)

	// Ensure target directory exists and is empty/clean?
	// Or just overwrite.
	if err := os.MkdirAll(targetDir, 0755); err != nil {
		return err
	}

	// Backup Directories
	for _, dir := range c.Directories {
		dirName := filepath.Base(dir)
		dest := filepath.Join(targetDir, "DIRS", dirName)
		if err := fsCopyDir(dir, dest); err != nil {
			// It might fail if dir doesn't exist (e.g. fresh install), acceptable?
			fmt.Printf("Warning: Failed to copy dir %s: %v\n", dir, err)
		}
	}

	// Backup Files
	for _, file := range c.Files {
		fileName := filepath.Base(file)
		dest := filepath.Join(targetDir, "FILES", fileName)
		// Ensure parent dir exists
		os.MkdirAll(filepath.Dir(dest), 0755)
		if err := fsCopyFile(file, dest); err != nil {
			fmt.Printf("Warning: Failed to copy file %s: %v\n", file, err)
		}
	}

	return nil
}

// LoadState restores the state from a named account folder.
func (c *FileSwapConfig) LoadState(accountName string) error {
	storageBase, err := c.GetStoragePath()
	if err != nil {
		return err
	}
	sourceDir := filepath.Join(storageBase, accountName)

	if _, err := os.Stat(sourceDir); os.IsNotExist(err) {
		return fmt.Errorf("account data not found for '%s'", accountName)
	}

	// First kill processes to unlock files
	c.KillProcesses()

	// Restore Directories
	for _, dir := range c.Directories {
		dirName := filepath.Base(dir)
		src := filepath.Join(sourceDir, "DIRS", dirName)
		// Check if we have a backup for this dir
		if _, err := os.Stat(src); err == nil {
			// Remove current dir to ensure clean state?
			// Or just overwrite. Removing is safer for 'swapping'.
			os.RemoveAll(dir)
			if err := fsCopyDir(src, dir); err != nil {
				return fmt.Errorf("failed to restore dir %s: %w", dir, err)
			}
		}
	}

	// Restore Files
	for _, file := range c.Files {
		fileName := filepath.Base(file)
		src := filepath.Join(sourceDir, "FILES", fileName)
		if _, err := os.Stat(src); err == nil {
			if err := fsCopyFile(src, file); err != nil {
				return fmt.Errorf("failed to restore file %s: %w", file, err)
			}
		}
	}

	return nil
}

// fsCopyFile copies a single file from src to dst
func fsCopyFile(src, dst string) error {
	sourceFile, err := os.Open(src)
	if err != nil {
		return err
	}
	defer sourceFile.Close()

	destFile, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer destFile.Close()

	_, err = io.Copy(destFile, sourceFile)
	return err
}

// fsCopyDir recursively copies a directory tree, attempting to preserve permissions.
func fsCopyDir(src string, dst string) error {
	src = filepath.Clean(src)
	dst = filepath.Clean(dst)

	si, err := os.Stat(src)
	if err != nil {
		return err
	}
	if !si.IsDir() {
		return fmt.Errorf("source is not a directory")
	}

	if _, err := os.Stat(dst); os.IsNotExist(err) {
		if err := os.MkdirAll(dst, si.Mode()); err != nil {
			return err
		}
	}

	entries, err := os.ReadDir(src)
	if err != nil {
		return err
	}

	for _, entry := range entries {
		srcPath := filepath.Join(src, entry.Name())
		dstPath := filepath.Join(dst, entry.Name())

		if entry.IsDir() {
			if err := fsCopyDir(srcPath, dstPath); err != nil {
				return err
			}
		} else {
			if err := fsCopyFile(srcPath, dstPath); err != nil {
				return err
			}
		}
	}
	return nil
}

// Helper to get Windows Env vars, returns empty on other OS
func GetWindowsEnv(key string) string {
	if runtime.GOOS == "windows" {
		return os.Getenv(key)
	}
	// Mock for Mac dev environment if needed, or return empty
	return ""
}
