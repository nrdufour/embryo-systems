/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.tools.launcher;

import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.impl.SimpleAdtManager;
import info.embryosys.adt.request.AdtRequest;
import info.embryosys.adt.request.RequestFactory;

/**
 * @author nrdufour
 * 
 */
public class ManagerTester {

	private AdtManager manager;

	/**
	 * 
	 */
	public ManagerTester() {
		this.manager = new SimpleAdtManager();
	}

	public void run() {
		String[] testValues = { "create Pont", "create Pont[Alma]",
				"create Pont.Hauteur", "create Pont-enjambe-Fleuve", };

		for (String value : testValues) {
			AdtRequest request = RequestFactory.analyzeString(value);
			this.manager.processRequest(request);
		}
	}

	/**
	 * @param args
	 */
	public static void main(String[] args) {
		// TODO Auto-generated method stub

		ManagerTester tester = new ManagerTester();

		tester.run();
	}

}
