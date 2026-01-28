using System;
using System.Diagnostics;
using System.IO;
using Microsoft.Win32;

namespace EpicSwitcher
{
    class Program
    {
        // Ключ реестра, отвечающий за текущий ID пользователя
        static string EpicRegistryPath = @"Software\Epic Games\Unreal Engine\Identifiers";
        static string EpicRegistryKey = "AccountId";

        static void Main(string[] args)
        {
            // Аргументы:
            // 0: action ("switch" или "save")
            // 1: accountName (Имя папки аккаунта)
            // 2: storagePath (Где лежат все аккаунты swch)

            if (args.Length < 3)
            {
                Console.WriteLine("Usage: EpicSwitcher.exe <action> <account_name> <storage_path>");
                Environment.Exit(1);
            }

            string action = args[0];
            string accountName = args[1];
            string baseStoragePath = args[2];
            string accountDir = Path.Combine(baseStoragePath, accountName);

            try
            {
                if (action == "switch")
                {
                    SwitchAccount(accountDir);
                }
                else if (action == "save")
                {
                    SaveAccount(accountDir);
                }
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Error: {ex.Message}");
                Environment.Exit(1);
            }
        }

        static void SwitchAccount(string sourceDir)
        {
            Console.WriteLine("Stopping Epic Games processes...");
            TerminateProcess("EpicGamesLauncher");
            TerminateProcess("EpicWebHelper");
            TerminateProcess("UnrealCEFSubProcess");

            if (!Directory.Exists(sourceDir))
            {
                throw new DirectoryNotFoundException($"Account directory not found: {sourceDir}");
            }

            // 1. Восстанавливаем GameUserSettings.ini (настройки лаунчера)
            string localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            string destConfigDir = Path.Combine(localAppData, "EpicGamesLauncher", "Saved", "Config", "WindowsEditor");
            string sourceIni = Path.Combine(sourceDir, "GameUserSettings.ini");

            if (File.Exists(sourceIni))
            {
                if (!Directory.Exists(destConfigDir)) Directory.CreateDirectory(destConfigDir);
                File.Copy(sourceIni, Path.Combine(destConfigDir, "GameUserSettings.ini"), true);
                Console.WriteLine("Config restored.");
            }

            // 2. Восстанавливаем ID пользователя в РЕЕСТР (Самое важное!)
            string sourceRegFile = Path.Combine(sourceDir, "AccountId.txt");
            if (File.Exists(sourceRegFile))
            {
                string accountId = File.ReadAllText(sourceRegFile).Trim();
                using (RegistryKey key = Registry.CurrentUser.CreateSubKey(EpicRegistryPath))
                {
                    if (key != null)
                    {
                        key.SetValue(EpicRegistryKey, accountId);
                        Console.WriteLine($"Registry AccountId updated to: {accountId}");
                    }
                }
            }
            else 
            {
                Console.WriteLine("Warning: AccountId.txt not found. Switch might fail.");
            }

            // 3. Чистим кэш, чтобы сбросить старую сессию
            Console.WriteLine("Clearing Epic cache...");
            ClearEpicCache(localAppData);

            Console.WriteLine("Success");
        }

        static void SaveAccount(string destDir)
        {
            if (!Directory.Exists(destDir)) Directory.CreateDirectory(destDir);

            Console.WriteLine("Saving account...");

            // 1. Сохраняем GameUserSettings.ini
            string localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            string sourceIni = Path.Combine(localAppData, "EpicGamesLauncher", "Saved", "Config", "WindowsEditor", "GameUserSettings.ini");
            
            if (File.Exists(sourceIni))
            {
                File.Copy(sourceIni, Path.Combine(destDir, "GameUserSettings.ini"), true);
            }

            // 2. Сохраняем ID из реестра
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(EpicRegistryPath))
            {
                if (key != null)
                {
                    object val = key.GetValue(EpicRegistryKey);
                    if (val != null)
                    {
                        File.WriteAllText(Path.Combine(destDir, "AccountId.txt"), val.ToString());
                        Console.WriteLine($"Saved Registry ID: {val}");
                    }
                }
            }
            Console.WriteLine("Saved");
        }

        static void TerminateProcess(string name)
        {
            foreach (var p in Process.GetProcessesByName(name))
            {
                try { p.Kill(); p.WaitForExit(1000); } catch { }
            }
        }

        static void ClearEpicCache(string localAppData)
        {
            // Список путей для очистки (на основе логики TcNo)
            string[] paths = {
                Path.Combine(localAppData, "Epic Games", "Epic Online Services", "UI Helper", "Cache"),
                Path.Combine(localAppData, "Epic Games", "EOSOverlay", "BrowserCache"),
                Path.Combine(localAppData, "EpicGamesLauncher", "Saved", "webcache"),
                Path.Combine(localAppData, "EpicGamesLauncher", "Saved", "webcache_4147"),
                Path.Combine(localAppData, "EpicGamesLauncher", "Saved", "webcache_4430")
            };

            foreach (var p in paths)
            {
                if (Directory.Exists(p))
                {
                    try { Directory.Delete(p, true); } catch { }
                }
            }
        }
    }
}