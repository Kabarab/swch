package models

import "strings"

// MergeLibraries сводит списки игр из разных источников (локальный скан
// манифестов, API магазинов, legacy-провайдеры) в единый: ключ — Platform+ID.
// Порядок первого появления сохраняется. Правила слияния:
//   - установленная запись побеждает (IsInstalled=true, ее ExePath);
//   - владельцы (AvailableOnAccounts) объединяются без дубликатов;
//   - пустые и плейсхолдерные имена ("Steam App 123") замещаются реальными;
//   - берется первая непустая иконка.
func MergeLibraries(lists ...[]LibraryGame) []LibraryGame {
	index := make(map[string]int)
	var result []LibraryGame

	for _, list := range lists {
		for _, g := range list {
			key := g.Platform + "|" + g.ID
			i, found := index[key]
			if !found {
				// Копия с собственным слайсом владельцев: источники не должны
				// делить память с результатом
				merged := g
				merged.AvailableOnAccounts = append([]AccountStat(nil), g.AvailableOnAccounts...)
				index[key] = len(result)
				result = append(result, merged)
				continue
			}
			mergeGameInto(&result[i], g)
		}
	}
	return result
}

func mergeGameInto(dst *LibraryGame, src LibraryGame) {
	if src.IsInstalled {
		dst.IsInstalled = true
		if src.ExePath != "" {
			dst.ExePath = src.ExePath
		}
	}
	if dst.ExePath == "" && src.ExePath != "" {
		dst.ExePath = src.ExePath
	}
	if isBetterName(dst.Name, src.Name) {
		dst.Name = src.Name
	}
	if dst.IconURL == "" {
		dst.IconURL = src.IconURL
	}
	if src.IsMacSupported {
		dst.IsMacSupported = true
	}
	if src.IsPinned {
		dst.IsPinned = true
	}
	for _, acc := range src.AvailableOnAccounts {
		if !hasOwner(dst.AvailableOnAccounts, acc) {
			dst.AvailableOnAccounts = append(dst.AvailableOnAccounts, acc)
		}
	}
}

// isBetterName: src лучше текущего dst, если dst пустое или плейсхолдер,
// а src — настоящее имя.
func isBetterName(dst, src string) bool {
	if src == "" || isPlaceholderName(src) {
		return false
	}
	return dst == "" || isPlaceholderName(dst)
}

func isPlaceholderName(name string) bool {
	return strings.HasPrefix(name, "Steam App ")
}

func hasOwner(list []AccountStat, acc AccountStat) bool {
	for _, a := range list {
		if a.AccountID == acc.AccountID && a.Username == acc.Username {
			return true
		}
	}
	return false
}
