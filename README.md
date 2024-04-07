# editorconfig-updater

C# code severity rule generator for .editorconfig files, in rust.

## Usage

```bash
editorconfig-updater path/to/editorconfig [roslyn-version [analyzers-version]]]
```

If no version is specified, the latest version is used. The version refers to a git branch, tag, or rev.

## How??

The tool uses 2 files present in the [dotnet/roslyn](https://github.com/dotnet/roslyn) repository, and one file present in the [dotnet/roslyn-analyzers](https://github.com/dotnet/roslyn-analyzers/) repository.

The first file is [ErrorCode.cs](https://github.com/dotnet/roslyn/blob/e59309f35553d53147088c01c5b7706d1e8cdec2/src/Compilers/CSharp/Portable/Errors/ErrorCode.cs), which is used to get the CS number and the corresponding rule name to look up in  the next file.

The second file is [CSharpResources.resx](https://github.com/dotnet/roslyn/blob/e59309f35553d53147088c01c5b7706d1e8cdec2/src/Compilers/CSharp/Portable/CSharpResources.resx), which is used to get the rule description.

The third file is [Microsoft.CodeAnalysis.NetAnalyzers.sarif](https://github.com/dotnet/roslyn-analyzers/blob/b7bb138809d5a7d31508fe0cd86d59ed4c864764/src/NetAnalyzers/Microsoft.CodeAnalysis.NetAnalyzers.sarif), which is used to get the code analysis CA info.

## why rust?

I wanted to learn rust, so I figured why not. It's also fun to write a tool for a completely different language.

## License

ISC (which is essentially MIT)
