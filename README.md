# GitHub Repository Sync

This simple script is focused on syncing a local repo to a remote main branch in github automatically. It can be added as a task or added to the startup tasks as a shortcut in windows.

The executable should sit in the same directory as the config.toml file to run things correctly.

Future updates will allow config selection of whether to create a visible terminal window or to allow a background task to run on the machine.

> Note this script works currently on MacOS but is only designed and tested for Windows.

## Script Process

- Reads the config file to identify the local and remote repo, as well as the personal access token if required.
- Checks if the commit hash/id for the remote repo matches the local git repo
- If not matching, it will go and pull the latest changes and update the local repo
- If they do match, it will continue to log the time since the last mis-match (defaulting first to when the script first ran) and check for any changes every 20 seconds (current default refresh)

## Running the Script on Windows Startup

1. Task Scheduler:

   Use Windows Task Scheduler to run your Rust script at startup.

   Steps:

   1. Open Task Scheduler from the Start Menu.
   2. Select “Create Basic Task” from the right panel.
   3. Name the task (e.g., “GitHub Sync Script”).
   4. Choose “When the computer starts” as the trigger.
   5. Select “Start a Program” as the action.
   6. Browse to the compiled Rust executable (.exe) and select it.
   7. Configure the task to run with highest privileges if needed.

2. Startup Folder:

   Place a shortcut to the Rust executable in the Windows Startup folder.

   Steps:

   1. Press Win + R and type shell:startup to open the Startup folder.
   2. Create a shortcut to your Rust script’s executable in this folder.

For all of the above, make sure to have the config.toml file with the filled in values in the same directory as the executible file that runs.
