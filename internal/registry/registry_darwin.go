//go:build darwin

package registry

// Заглушки типов для совместимости
type Key uintptr

const (
	CLASSES_ROOT  Key = 0
	CURRENT_USER  Key = 1
	LOCAL_MACHINE Key = 2
)

// ExpandRegistryAbbreviation возвращает ноль на macos
func ExpandRegistryAbbreviation(abv string) Key {
	return 0
}

// GetStringValue всегда возвращает пустоту на macos
func GetStringValue(fullPath, valueName string) (string, error) {
	return "", nil
}

// SetStringValue ничего не делает на macos
func SetStringValue(fullPath, valueName, value string) error {
	return nil
}
