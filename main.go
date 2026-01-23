package main

import (
	"fmt"
	"log"
	
	"myproject/internal/registry"
	"myproject/internal/processes"
)

func SwitchSteamAccount(username string) {
	fmt.Println("Closing Steam...")
	processes.KillProcess("steam.exe")

	fmt.Printf("Switching to %s...\n", username)
	
	keyPath := `HKCU\Software\Valve\Steam`
	
	// Устанавливаем авто-логин
	err := registry.SetStringValue(keyPath, "AutoLoginUser", username)
	if err != nil {
		log.Printf("Error setting AutoLoginUser: %v", err)
	}
	
	// Важно: сбросить RememberPassword в 1, чтобы не просил пароль (если токен жив)
	registry.SetStringValue(keyPath, "RememberPassword", "1")

	fmt.Println("Starting Steam...")
	// Путь к Steam лучше тоже брать из реестра
	steamPath, _ := registry.GetStringValue(keyPath, "SteamPath")
	if steamPath != "" {
		processes.StartProgram(steamPath + "/steam.exe")
	}
}

func main() {
	// Пример использования
	SwitchSteamAccount("MyGameAccount")
}