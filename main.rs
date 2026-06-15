fn main(){
	let mut buffer=String::new();
	//let mut results=String::new();
	let mut terminal=StringTerminal::new(24,80);

	println!("Terminal started. Type commands or '\\quit' to kill current session.");
	terminal.enable_log();

	loop{
		buffer.clear();
		io::stdin().read_line(&mut buffer).unwrap();

		let clean=buffer.trim();

		if clean=="\\quit"{
			// Sending a literal 0x03 byte over a PTY triggers a native Ctrl+C (SIGINT)
			terminal.put_input("\x03");
			//println!("Process killed.");
			continue;
		}
		if clean=="\\quit terminal"{
			//println!("Terminal stopped.");
			break;
		}
		terminal.put_input(&format!("{clean}\n"));
		thread::sleep(Duration::from_millis(200));

		let output=terminal.take_snapshot();
		if !output.is_empty() {
			print!("{}", output);
			io::stdout().flush().ok();
		}
	}

	let filename=format!("string-terminal-record_{}",rand::random::<u64>());
	std::fs::write(filename,terminal.log()).unwrap();
}
impl StringTerminal{
	/// disable logging
	pub fn disable_log(&mut self){self.log=None}
	/// enable logging
	pub fn enable_log(&mut self){
		if self.log.is_none(){self.log=Some(String::new())}
	}
	/// start session if not started
	pub fn ensure_session(&mut self)->Result<&mut Box<dyn Write+Send>,String>{
		if self.writer.is_some(){return Ok(self.writer.as_mut().unwrap())}

		let system=NativePtySystem::default();
		let pair=system.openpty(PtySize{rows:self.rows,cols:self.cols,pixel_width:0,pixel_height:0}).map_err(|e|e.to_string())?;

		let cmd=if cfg!(target_os="windows"){"cmd"}else{"sh"};
		let builder=CommandBuilder::new(cmd);

		let _child=pair.slave.spawn_command(builder).map_err(|e|e.to_string())?;

		// Take ownership of the Master side reader and writer pipes
		let reader=pair.master.try_clone_reader().map_err(|e|e.to_string())?;
		let writer=pair.master.take_writer()     .map_err(|e|e.to_string())?;

		let outbuf=Arc::clone(&self.output);
		thread::spawn(move||{
			let mut chunk=[0u8;1024];
			let output=outbuf;
			let mut reader=reader;

			while let Ok(n)=reader.read(&mut chunk)&&n>0{
				output.lock().unwrap().process(&chunk[..n]);
			}
		});

		self.writer=Some(writer);
		Ok(self.writer.as_mut().unwrap())
	}
	/// check if log is enabled
	pub fn is_log_enabled(&self)->bool{self.log.is_some()}
	/// Helper method to query if the sub-application is using the alternate layout screen
	pub fn is_tui_active(&self)->bool{
		let lock=self.output.lock().unwrap();
		lock.screen().alternate_screen()
	}
	/// references the log state. "" if empty or not enabled
	pub fn log(&self)->&str{self.log.as_deref().unwrap_or_default()}
	pub fn new(rows:impl TryInto<u16>,cols:impl TryInto<u16>)->Self{
		let (rows,cols)=rows.try_into().ok().zip(cols.try_into().ok()).expect("must be able to cast rows and cols to u16");
		Self{
			log:None,
			output:Arc::new(Mutex::new(vt100::Parser::new(rows,cols,0))),
			writer:None,
			rows,
			cols,
		}
	}
	/*
	/// Explicitly kills the current shell instance
	pub fn kill_session(&mut self){

	}*/
	/// inputs the string to the terminal
	pub fn put_input(&mut self,input:&str){
		if input.is_empty() {
			return;
		}
		if let Some(log)=self.log.as_mut(){log.push_str(input)}

		let input=if self.is_tui_active(){
			input.replace("\r\n","\n").replace('\n',"\r")
		} else {
			input.to_string()
		};
		let mut try_write=|input:&str|{
			let writer=self.ensure_session().map_err(|e|e.to_string())?;

			writer.write_all(input.as_bytes()).map_err(|e|e.to_string())?;
			writer.flush().map_err(|e|e.to_string())?;
			Ok::<(),String>(())
		};

		if let Err(e)=try_write(&input){
			self.writer=None;
			let mut lock=self.output.lock().unwrap();
			lock.process(format!("Shell failed: {e}\n").as_bytes());
		}
	}
	/// enable or disable logging
	pub fn set_log_enabled(&mut self,enable:bool){
		if enable{self.enable_log()}else{self.disable_log()}
	}
	/// get the current information
	pub fn take_snapshot(&mut self)->String{
		let lock=self.output.lock().unwrap();
		let screen=lock.screen();

		let mut rendered=String::new();
		for line in screen.rows(0,self.cols){
			rendered.push_str(&line);
			rendered.push('\n');
		}

		if let Some(log)=self.log.as_mut(){log.push_str(&rendered)}
		rendered
	}
}

pub fn test00(){
	let mut terminal = StringTerminal::new(24, 80); // Create a virtual 24x80 terminal window
	terminal.enable_log();

	println!("--- Booting Session ---");
	thread::sleep(Duration::from_millis(100));
	println!("{}", terminal.take_snapshot());

	// Transaction 1: Run nano with a new file name
	println!("--- Transaction: Starting Nano ---");
	terminal.put_input("nano document.txt\n");
	thread::sleep(Duration::from_millis(200));
	println!("{}", terminal.take_snapshot());

	// Transaction 2: Type some text inside the virtual nano window
	println!("--- Transaction: Typing text inside nano ---");
	terminal.put_input("Hello, this text is typed inside nano programmatically!\n");
	thread::sleep(Duration::from_millis(100));
	println!("{}", terminal.take_snapshot());

	// Transaction 3: Exit nano cleanly via standard shortcuts (Ctrl+O then Ctrl+X)
	println!("--- Transaction: Saving and Exiting Nano ---");
	terminal.put_input("\x0F"); // ASCII 15 = Ctrl+O (WriteOut)
	thread::sleep(Duration::from_millis(200));
	terminal.put_input("\n");   // Press Enter to confirm filename
	thread::sleep(Duration::from_millis(200));
	terminal.put_input("\x18"); // ASCII 24 = Ctrl+X (Exit)
	thread::sleep(Duration::from_millis(200));

	// Verify we are safely back in the main shell prompt
	println!("--- Final Screen State ---");
	println!("{}", terminal.take_snapshot());

	let filename=format!("string-terminal-record_{}",rand::random::<u64>());
	std::fs::write(filename,terminal.log()).unwrap();
}

pub struct StringTerminal{log:Option<String>,output:Arc<Mutex<Parser>>,writer:Option<Box<dyn Write+Send>>,cols:u16,rows:u16}

//use command_group::{CommandGroup,GroupChild};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::{
	io::{Read,Write,self},sync::{Arc,Mutex},thread,time::Duration
};
use vt100::Parser;
//use token_dict::UTF8CharIter;
