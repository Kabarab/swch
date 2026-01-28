using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

namespace EpicSwitcher
{
    class Program
    {
        static void Main(string[] args)
        {
            // Ожидаемые аргументы:
            // 0: action ("switch" или "save")
            // 1: путь к папке с сохраненным аккаунтом (Source для switch / Dest для save)
            // 2: путь к системной папке Epic Data (Dest для switch / Source для save)

            if (args.Length < 3)
            {
                Console.WriteLine("Usage: EpicSwitcher.exe <action> <account_path> <system_path>");
                Environment.Exit(1);
            }

            string action = args[0];
            string accountPath = args[1];
            string systemPath = args[2];

            try
            {
                if (action == "switch")
                {
                    SwitchAccount(accountPath, systemPath);
                }
                else if (action == "save")
                {
                    SaveAccount(systemPath, accountPath);
                }
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Error: {ex.Message}");
                Environment.Exit(1);
            }
        }

        static void SwitchAccount(string sourceDir, string destDir)
        {
            Console.WriteLine("Stopping Epic Games Launcher...");
            TerminateProcess("EpicGamesLauncher");
            // Иногда Epic запускает отдельные процессы для UnrealEngine, можно добавить и их
            TerminateProcess("EpicWebHelper"); 

            Console.WriteLine("Clearing current session...");
            CleanDirectory(destDir);

            Console.WriteLine("Restoring account...");
            CopyDirectory(sourceDir, destDir);
            
            Console.WriteLine("Success.");
        }

        static void SaveAccount(string sourceDir, string destDir)
        {
            // При сохранении убивать процесс не обязательно, но желательно для целостности данных
            // TerminateProcess("EpicGamesLauncher"); 

            Console.WriteLine("Saving account...");
            CleanDirectory(destDir); // Очищаем папку бэкапа перед записью
            CopyDirectory(sourceDir, destDir);
            Console.WriteLine("Saved.");
        }

        // Аналог KillGameProcesses из TcNo
        static void TerminateProcess(string processName)
        {
            foreach (var process in Process.GetProcessesByName(processName))
            {
                try
                {
                    process.Kill();
                    process.WaitForExit(3000); // Ждем до 3 секунд
                }
                catch (Exception e)
                {
                    Console.WriteLine($"Warning: Could not kill {processName}: {e.Message}");
                }
            }
        }

        // Аналог ClearCurrentLoginBasic из TcNo
        static void CleanDirectory(string path)
        {
            if (Directory.Exists(path))
            {
                DirectoryInfo di = new DirectoryInfo(path);
                foreach (FileInfo file in di.GetFiles())
                {
                    file.Delete();
                }
                foreach (DirectoryInfo dir in di.GetDirectories())
                {
                    dir.Delete(true);
                }
            }
            else
            {
                Directory.CreateDirectory(path);
            }
        }

        // Аналог BasicCopyInAccount из TcNo (рекурсивное копирование)
        static void CopyDirectory(string sourceDir, string destDir)
        {
            DirectoryInfo dir = new DirectoryInfo(sourceDir);

            if (!dir.Exists)
                throw new DirectoryNotFoundException($"Source directory not found: {sourceDir}");

            DirectoryInfo[] dirs = dir.GetDirectories();
            Directory.CreateDirectory(destDir);

            foreach (FileInfo file in dir.GetFiles())
            {
                string tempPath = Path.Combine(destDir, file.Name);
                file.CopyTo(tempPath, true);
            }

            foreach (DirectoryInfo subdir in dirs)
            {
                string tempPath = Path.Combine(destDir, subdir.Name);
                CopyDirectory(subdir.FullName, tempPath);
            }
        }
    }
}