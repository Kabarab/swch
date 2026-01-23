package registry

import (
	"fmt"
	"strings"

	"golang.org/x/sys/windows/registry"
)

func ExpandRegistryAbbreviation(abv string) registry.Key {
	switch abv {
	case "HKCR":
		return registry.CLASSES_ROOT
	case "HKCU":
		return registry.CURRENT_USER
	case "HKLM":
		return registry.LOCAL_MACHINE
	default:
		return 0
	}
}

func GetStringValue(fullPath, valueName string) (string, error) {
	root, subPath := splitRootAndPath(fullPath)
	k, err := registry.OpenKey(root, subPath, registry.QUERY_VALUE)
	if err != nil {
		return "", err
	}
	defer k.Close()

	val, _, err := k.GetStringValue(valueName)
	return val, err
}

func SetStringValue(fullPath, valueName, value string) error {
	root, subPath := splitRootAndPath(fullPath)
	k, _, err := registry.CreateKey(root, subPath, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	return k.SetStringValue(valueName, value)
}

func splitRootAndPath(p string) (registry.Key, string) {
	parts := strings.SplitN(p, "\\", 2)
	if len(parts) < 2 {
		return registry.CURRENT_USER, p
	}
	return ExpandRegistryAbbreviation(parts[0]), parts[1]
}