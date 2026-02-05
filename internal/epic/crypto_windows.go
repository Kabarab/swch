package epic

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
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

func FindEpicConfigDir() (string, error) {
	localAppData := os.Getenv("LOCALAPPDATA")
	if localAppData == "" {
		return "", fmt.Errorf("LOCALAPPDATA environment variable not set")
	}
	// Epic Games Store stores data in %LOCALAPPDATA%\EpicGamesLauncher\Saved\Data
	path := filepath.Join(localAppData, "EpicGamesLauncher", "Saved", "Data")
	return path, nil
}

// EpicTokenFile represents the structure of the .dat file (JSON)
type EpicTokenFile struct {
	RememberMe   string `json:"RememberMe,omitempty"`
	RefreshToken string `json:"RefreshToken,omitempty"`
}

func ExtractTokens() (string, error) {
	dir, err := FindEpicConfigDir()
	if err != nil {
		return "", err
	}

	// Iterate over .dat files in the directory
	entries, err := os.ReadDir(dir)
	if err != nil {
		return "", err
	}

	for _, entry := range entries {
		if filepath.Ext(entry.Name()) == ".dat" {
			path := filepath.Join(dir, entry.Name())
			data, err := os.ReadFile(path)
			if err != nil {
				continue
			}

			// Try to decrypt
			decrypted, err := DecryptData(data)
			if err != nil {
				// Maybe it wasn't encrypted or failed
				continue
			}

			// Try to parse as JSON
			var tokenFile map[string]interface{}
			if err := json.Unmarshal(decrypted, &tokenFile); err == nil {
				// Look for RememberMe or similar keys
				if val, ok := tokenFile["RememberMe"].(string); ok && val != "" {
					return val, nil
				}
			}
		}
	}

	return "", fmt.Errorf("no valid token found in Epic Games data")
}

// TryImportNativeSession attempts to read the Epic Games Launcher's local session
// and sign in using the extracted token.
func (c *EpicClient) TryImportNativeSession() error {
	fmt.Println("[Epic] Attempting to import native session...")
	token, err := ExtractTokens()
	if err != nil {
		fmt.Printf("[Epic] Failed to extract native tokens: %v\n", err)
		return err
	}

	fmt.Println("[Epic] Found native token, attempting login...")
	// The "RememberMe" token can sometimes be used as a refresh token or in a specific flow.
	// We'll try using it as a refresh token first.
	if err := c.LoginWithRefreshToken(token); err != nil {
		fmt.Printf("[Epic] Login with extracted token failed: %v\n", err)
		return err
	}

	fmt.Println("[Epic] Successfully imported and refreshed native session")
	return nil
}
