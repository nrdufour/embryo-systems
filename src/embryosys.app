%% This is the application resource file (.app file) for the 'base'
%% application.
{application, embryosys,
	[{description, "Embryo Systems"},
	 {vsn, "0.1"},
	 {modules, [embryosys, embryosys_sup,
		    adtm_class, adtm_attribute, adtm_link, adtm_object,
		    storage_server]},
	 {registered, [adtm_class, adtm_attribute, adtm_link, adtm_object, storage_server, embryosys_sup]},
	 {applications, [kernel,stdlib]},
	 {mod, {embryosys, []}},
	 {start_phases, []}
]}.

