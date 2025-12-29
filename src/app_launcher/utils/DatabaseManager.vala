namespace AppLauncher {
    public class DatabaseManager : Object {
        private static DatabaseManager? _instance = null;
        private Sqlite.Database? db = null;
        
        private DatabaseManager() {
            init_database();
        }
        
        public static DatabaseManager get_instance() {
            if (_instance == null) {
                _instance = new DatabaseManager();
            }
            return _instance;
        }
        
        private void init_database() {
            var data_dir = Environment.get_user_data_dir();
            var nosh_data_dir = Path.build_filename(data_dir, "nosh");
            
            // Create directory if it doesn't exist
            var dir = File.new_for_path(nosh_data_dir);
            try {
                dir.make_directory_with_parents();
            } catch (Error e) {
                if (!(e is IOError.EXISTS)) {
                    warning("Failed to create data directory: %s", e.message);
                }
            }
            
            var db_path = Path.build_filename(nosh_data_dir, "app_launcher.db");
            
            if (Sqlite.Database.open(db_path, out db) != Sqlite.OK) {
                warning("Failed to open database: %s", db != null ? db.errmsg() : "unknown error");
                db = null;
                return;
            }
            
            // Create table
            string create_table = """
                CREATE TABLE IF NOT EXISTS app_usage (
                    desktop_id TEXT PRIMARY KEY,
                    frequency INTEGER DEFAULT 0,
                    last_used INTEGER DEFAULT 0
                )
            """;
            
            if (db.exec(create_table) != Sqlite.OK) {
                warning("Failed to create table: %s", db.errmsg());
            }
        }
        
        public int get_frequency(string desktop_id) {
            if (db == null) return 0;
            
            Sqlite.Statement stmt;
            string query = "SELECT frequency FROM app_usage WHERE desktop_id = ?";
            
            if (db.prepare_v2(query, -1, out stmt) != Sqlite.OK) {
                warning("Failed to prepare statement: %s", db.errmsg());
                return 0;
            }
            
            stmt.bind_text(1, desktop_id);
            
            if (stmt.step() == Sqlite.ROW) {
                return stmt.column_int(0);
            }
            
            return 0;
        }
        
        public void increment_frequency(string desktop_id) {
            if (db == null) return;
            
            string query = """
                INSERT INTO app_usage (desktop_id, frequency, last_used)
                VALUES (?, 1, ?)
                ON CONFLICT(desktop_id) DO UPDATE SET
                    frequency = frequency + 1,
                    last_used = ?
            """;
            
            Sqlite.Statement stmt;
            if (db.prepare_v2(query, -1, out stmt) != Sqlite.OK) {
                warning("Failed to prepare statement: %s", db.errmsg());
                return;
            }
            
            int64 now = (int64) GLib.get_real_time() / 1000000;
            stmt.bind_text(1, desktop_id);
            stmt.bind_int64(2, now);
            stmt.bind_int64(3, now);
            
            if (stmt.step() != Sqlite.DONE) {
                warning("Failed to increment frequency: %s", db.errmsg());
            }
        }
    }
}
