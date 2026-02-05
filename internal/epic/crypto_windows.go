package epic

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"swch/internal/sys"
	"syscall"
	"unsafe"
)

var (
	crypt32           = syscall.NewLazyDLL("crypt32.dll")
	procUnprotectData = crypt32.NewProc("CryptUnprotectData")
)

type DATA_BLOB struct {
	cbData uint32
	pbData *byte
}

func DecryptData(data []byte) ([]byte, error) {
	var inBlob, outBlob DATA_BLOB

	inBlob.cbData = uint32(len(data))
	inBlob.pbData = &data[0]

	ret, _, err := procUnprotectData.Call(
		uintptr(unsafe.Pointer(&inBlob)),
		0,
		0,
		0,
		0,
		0,
		uintptr(unsafe.Pointer(&outBlob)),
	)

	if ret == 0 {
		return nil, fmt.Errorf("CryptUnprotectData failed: %w", err)
	}
	defer syscall.LocalFree(syscall.Handle(uintptr(unsafe.Pointer(outBlob.pbData))))

	// copy data to Go slice
	decrypted := make([]byte, outBlob.cbData)
	// unsafe copy
	src := unsafe.Slice(outBlob.pbData, outBlob.cbData)
	copy(decrypted, src)

	return decrypted, nil
}

// ExtractTokens returns the raw token string (RememberMe) from Epic's encrypted data.
// It tries to find it in any .dat file in the data directory.
func ExtractTokens() (string, error) {
	// Use sys package for consistent path
	dir := sys.GetEpicAuthDataDir()

	fmt.Printf("[EpicImport] Сканирование токенов в: %s\n", dir)

	// Iterate over .dat files in the directory
	entries, err := os.ReadDir(dir)
	if err != nil {
		return "", fmt.Errorf("не удалось прочитать директорию %s: %w", dir, err)
	}

	var checkedFiles []string
	var lastErr error

	for _, entry := range entries {
		if filepath.Ext(entry.Name()) == ".dat" {
			path := filepath.Join(dir, entry.Name())
			checkedFiles = append(checkedFiles, entry.Name())

			data, err := os.ReadFile(path)
			if err != nil {
				lastErr = fmt.Errorf("ошибка чтения: %w", err)
				continue
			}

			// Try to decrypt
			decrypted, err := DecryptData(data)
			if err != nil {
				// Normal, some files might not be DPAPI encrypted or use different keys
				// lastErr = fmt.Errorf("decrypt fail: %w", err) // Too verbose
				continue
			}

			// Try to parse as JSON
			var tokenFile map[string]interface{}
			if err := json.Unmarshal(decrypted, &tokenFile); err == nil {
				// Look for RememberMe
				if val, ok := tokenFile["RememberMe"].(string); ok && val != "" {
					fmt.Printf("[EpicImport] Найден RememberMe в %s\n", entry.Name())
					return val, nil
				}
				// Also check for "RefreshToken" just in case schema differs
				if val, ok := tokenFile["RefreshToken"].(string); ok && val != "" {
					fmt.Printf("[EpicImport] Найден RefreshToken в %s\n", entry.Name())
					return val, nil
				}
			}
		}
	}

	return "", fmt.Errorf("просканировано файлов: %d (%v), валидный токен не найден. Последняя ошибка: %v", len(checkedFiles), checkedFiles, lastErr)
}

// TryImportNativeSession attempts to read the Epic Games Launcher's local session
// and sign in using the extracted token.
func (c *EpicClient) TryImportNativeSession() error {
	fmt.Println("[Epic] Попытка импорта нативной сессии...")
	token, err := ExtractTokens()
	if err != nil {
		fmt.Printf("[Epic] Не удалось извлечь нативные токены: %v\n", err)
		return err
	}

	fmt.Println("[Epic] Нативный токен найден, попытка входа...")
	// The "RememberMe" token can sometimes be used as a refresh token
	if err := c.LoginWithRefreshToken(token); err != nil {
		fmt.Printf("[Epic] Вход с извлеченным токеном не удался: %v\n", err)
		return fmt.Errorf("вход по токену не удался: %w", err)
	}

	fmt.Println("[Epic] Успешный импорт и обновление нативной сессии")
	return nil
}
