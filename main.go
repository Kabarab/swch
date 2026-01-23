package main

import (
	"embed"
	"swch/internal/app"

	"github.com/wailsapp/wails/v2"
	"github.com/wailsapp/wails/v2/pkg/options"
	"github.com/wailsapp/wails/v2/pkg/options/assetserver"
)

var assets embed.FS

func main() {
	myApp := app.NewApp()

	err := wails.Run(&options.App{
		Title:  "swch",
		Width:  1024,
		Height: 768,
		AssetServer: &assetserver.Options{
			Assets: assets,
		},
		BackgroundColour: &options.RGBA{R: 18, G: 18, B: 18, A: 255},
		OnStartup:        myApp.Startup,
		Bind: []interface{}{
			myApp,
		},
	})

	if err != nil {
		println("Error:", err.Error())
	}
}