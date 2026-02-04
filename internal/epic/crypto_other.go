//go:build !windows

package epic

import "fmt"

func DecryptData(data []byte) ([]byte, error) {
	return nil, fmt.Errorf("not supported on this platform")
}

func FindEpicConfigDir() (string, error) {
	return "", fmt.Errorf("not supported on this platform")
}

func ExtractTokens() (string, error) {
	return "", fmt.Errorf("not supported on this platform")
}
