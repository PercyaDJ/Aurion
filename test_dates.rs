use chrono::NaiveDateTime;

fn main() {
    let session_name = "2026-03-05_21-30";
    let start_fmt = NaiveDateTime::parse_from_str(&format!("{}_00", session_name), "%Y-%m-%d_%H-%M_%S");
    println!("Session parsed: {:?}", start_fmt);
    
    let filename = "aurora_20260305_213500_00001.jpg";
    let ts_str = &filename[7..22];
    println!("ts_str: {}", ts_str);
    let file_dt = NaiveDateTime::parse_from_str(ts_str, "%Y%m%d_%H%M%S");
    println!("File parsed: {:?}", file_dt);
    
    if let (Ok(s_dt), Ok(f_dt)) = (start_fmt, file_dt) {
        let margin = s_dt - chrono::Duration::try_minutes(2).unwrap();
        println!("File >= margin? {}", f_dt >= margin);
    }
}
