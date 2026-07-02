fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icons/claude_clone.ico");
        resource.set("FileDescription", env!("CARGO_PKG_DESCRIPTION"));
        resource.set("ProductName", "Claude Clone");
        resource.set("OriginalFilename", "claude_clone.exe");
        resource
            .compile()
            .expect("failed to compile Windows application resources");
    }
}
