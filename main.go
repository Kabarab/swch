package main

import (
	"embed"
	"swch/internal/app"

	"github.com/wailsapp/wails/v2"
	"github.com/wailsapp/wails/v2/pkg/options"
	"github.com/wailsapp/wails/v2/pkg/options/assetserver"
)

//go:embed all:frontend/dist
var assets embed.FS

func main() {
	// Создаем экземпляр приложения
	mainApp := app.NewApp()

	err := wails.Run(&options.App{
		Title:  "swch",
		Width:  1024,
		Height: 768,
		AssetServer: &assetserver.Options{
			Assets: assets,
		},
		BackgroundColour: &options.RGBA{R: 27, G: 38, B: 56, A: 1},
		OnStartup:        mainApp.Startup,
		Bind: []interface{}{
			mainApp, // Привязываем методы для доступа из JS
		},
	})

	if err != nil {
		println("Error:", err.Error())
	}
}
