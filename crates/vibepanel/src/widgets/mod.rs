@@
             "media" => {
                 let cfg = MediaConfig::from_entry(entry);
                 let media = MediaWidget::new(cfg);
                 let root = media.widget().clone().upcast::<Widget>();
                 let edge_interaction = media.edge_interaction();
                 Some(BuiltWidget::new(root, media).with_edge_interaction(edge_interaction))
             }
+            "launcher" => {
+                let cfg = LauncherConfig::from_entry(entry);
+                let widget = LauncherWidget::new(cfg);
+                let root = widget.widget().clone().upcast::<Widget>();
+                Some(BuiltWidget::new(root, widget))
+            }
             "spacer" => {
                 let cfg = SpacerConfig::from_entry(entry);
                 let spacer = SpacerWidget::new(cfg);
                 let root = spacer.widget().clone().upcast::<Widget>();
                 Some(BuiltWidget::new(root, spacer))
             }
@@
