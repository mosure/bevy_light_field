use std::process::Command;


#[derive(Debug, Clone)]
pub struct FfmpegArgs {
    pub mp4_path: String,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    pub interpolation: String,
    pub output_directory: String,
}

impl FfmpegArgs {
    pub fn run(&self) -> std::io::Result<()> {
        let output_pattern = format!("{}/%d.png", self.output_directory);

        let resolution = format!("{}x{}", self.width, self.height);

        let _ = Command::new("ffmpeg")
            .arg("-i").arg(&self.mp4_path)
            .arg("-r").arg(self.fps.to_string())
            .arg("-s").arg(&resolution)
            .arg("-sws_flags").arg(&self.interpolation)
            .arg("-vf").arg(format!("fps=fps={}", self.fps))
            .arg(&output_pattern)
            .spawn()?
            .wait()?;

        Ok(())
    }
}
