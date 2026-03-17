Dot Agent - Ferritor transition:

GOAL: Translate DotAgent, a Claude/Codex skill, hook and related viewer into a Markdown+basic file editor
WHAT ALREADY EXISTS: DotAgent is already an EGui based dual-pane app that includes a tree-view with selectable objects and the ability to run batch commands. The editor pane already uses Synect to render markdown (without preview) and allow for the editing thereof.
WHAT IT NEEDS (Housework): All repo and filenames need to be renamed to Ferritor from DotAgent (or dotagent, or however it is currently capitalized)
WHAT IT NEEDS (Functionality): The treeview obviously needs to be expanded from a hard-wired scan of folders to a folder view with a filepicker icon and the ability to move among folders on the system
The editor also needs to increase the syntect highlighting to more file types (ideally, js and rs to start, with a fallback to no formatting) with a v2 including a formatting toolbar similar to the one in ~/repos/ferrite (a similar EGui-based app)
In terms of UI and commands, it needs:

The removal of the re-scan button and functionality
A folder icon in the same row as the filter field in the left panel (whose textbox would then be re-sized to fit the button)that handles a filepicker dialog for directory search, wired to a ctrl-shift-o keybind
The removal of all status bar indicators except pane. Currently it's Sidebar | Viewer. It should be Files (Just a sidebar rename) and then a conditional (Editing |Viewing) based on being in the right pane in either viewing mode or editing mode.
A v2 can include word count and character counts.
It would be inordinately helpful if there was a state file created like the one at ~/syncthing/.ferrite for basics like "recent files added" but the actual functionality adds (recently opened, etc.) wouldn't be mandatory
And it would also be inordinately helpful if the tree view had a ... row above the current directory structure as opened. This would simply be a "move up one directory command" and would be wired to ctrl-up-arrow.